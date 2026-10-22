//! The commands the window calls. Each is async: Tauri runs a plain command
//! on the main thread, which is the thread that draws the window, and no read
//! of a repository - let alone a run of git - may hold that up.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use furca_core::{HeadSummary, Refs, Remote, Repository, Standing, Stash, Submodule};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::maintenance;
use crate::state::{self, Tab, WindowState};

/// The window's store and the directory it is saved in.
pub struct Store {
    pub dir: PathBuf,
    pub state: Mutex<WindowState>,
}

impl Store {
    /// Applies `change` to the state, saves it and returns what the window
    /// draws next. A failed save is reported, not hidden: the change stands
    /// on the screen but would not survive a restart.
    fn change<T>(
        &self,
        change: impl FnOnce(&mut WindowState) -> T,
    ) -> Result<(WindowState, T), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "the window's state is poisoned")?;
        let out = change(&mut state);
        state::save(&self.dir, &state)
            .map_err(|e| format!("could not save the window's state: {e}"))?;
        Ok((state.clone(), out))
    }
}

/// A repository that did not open, and why.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Failure {
    pub path: String,
    pub reason: String,
}

/// The state after opening several repositories, with the ones that did not
/// open: a session of eighteen opens the seventeen that are there.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Opened {
    pub state: WindowState,
    pub failed: Vec<Failure>,
}

/// What the left column shows of a repository.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Sidebar {
    pub head: HeadSummary,
    pub refs: Refs,
    pub remotes: Vec<Remote>,
    pub stashes: Vec<Stash>,
    pub submodules: Vec<Submodule>,
}

/// The repositories the owner's record keeps - `furca line`'s list - or why
/// there is none here.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Line {
    Kept { tabs: Vec<Tab> },
    Unavailable { reason: String },
}

/// The tab for the repository that contains `path`: a path inside a working
/// tree opens the repository around it, as `git -C` does.
fn tab_for(path: &Path) -> Result<Tab, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let root = repo.workdir().unwrap_or_else(|| repo.git_dir());
    let root = state::normalize(root).map_err(|e| format!("{}: {e}", root.display()))?;
    Ok(Tab::at(&root))
}

/// Runs `work` on a thread of its own and waits for it without holding up
/// the async runtime.
async fn off_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("the work stopped: {e}"))?
}

#[tauri::command]
pub async fn window_state(store: State<'_, Store>) -> Result<WindowState, String> {
    store
        .state
        .lock()
        .map(|state| state.clone())
        .map_err(|_| "the window's state is poisoned".to_owned())
}

#[tauri::command]
pub async fn open_tab(
    app: AppHandle,
    store: State<'_, Store>,
    path: String,
) -> Result<WindowState, String> {
    let tab = off_thread(move || tab_for(Path::new(&path))).await?;
    maintenance::commit_graph(&app, &tab.path);
    Ok(store.change(|state| state.open(tab))?.0)
}

/// Opens every one of `tabs` in place of the open tabs, the one at `active`
/// in front: a session, or the line.
async fn open_all(
    app: &AppHandle,
    store: &Store,
    tabs: Vec<Tab>,
    active: Option<usize>,
) -> Result<Opened, String> {
    let wanted = active
        .and_then(|index| tabs.get(index))
        .map(|tab| tab.path.clone());
    let read = off_thread(move || {
        Ok(tabs
            .into_iter()
            .map(|tab| (tab.path.clone(), tab_for(Path::new(&tab.path))))
            .collect::<Vec<_>>())
    })
    .await?;

    let mut open = Vec::new();
    let mut failed = Vec::new();
    let mut front = None;
    for (path, tab) in read {
        match tab {
            Ok(tab) => {
                if wanted.as_deref() == Some(path.as_str()) {
                    front = Some(open.len());
                }
                maintenance::commit_graph(app, &tab.path);
                open.push(tab);
            }
            Err(reason) => failed.push(Failure { path, reason }),
        }
    }
    let (state, ()) = store.change(|state| state.replace(open, front))?;
    Ok(Opened { state, failed })
}

#[tauri::command]
pub async fn close_tab(store: State<'_, Store>, index: usize) -> Result<WindowState, String> {
    Ok(store.change(|state| state.close(index))?.0)
}

#[tauri::command]
pub async fn activate_tab(store: State<'_, Store>, index: usize) -> Result<WindowState, String> {
    Ok(store.change(|state| state.activate(index))?.0)
}

#[tauri::command]
pub async fn move_tab(
    store: State<'_, Store>,
    from: usize,
    to: usize,
) -> Result<WindowState, String> {
    Ok(store.change(|state| state.move_tab(from, to))?.0)
}

#[tauri::command]
pub async fn save_session(store: State<'_, Store>, name: String) -> Result<WindowState, String> {
    let (state, saved) = store.change(|state| state.save_session(&name))?;
    if saved {
        Ok(state)
    } else {
        Err("a session needs a name and at least one open tab".to_owned())
    }
}

#[tauri::command]
pub async fn open_session(
    app: AppHandle,
    store: State<'_, Store>,
    name: String,
) -> Result<Opened, String> {
    let session = store
        .state
        .lock()
        .map_err(|_| "the window's state is poisoned")?
        .session(&name)
        .cloned()
        .ok_or_else(|| format!("there is no session called {name}"))?;
    open_all(&app, &store, session.tabs, session.active).await
}

#[tauri::command]
pub async fn delete_session(store: State<'_, Store>, name: String) -> Result<WindowState, String> {
    Ok(store.change(|state| state.delete_session(&name))?.0)
}

/// The line, read from the record as `furca line` reads it.
#[tauri::command]
pub async fn line() -> Result<Line, String> {
    off_thread(|| {
        Ok(match furca_release::Rigger.repositories() {
            Ok(kept) => Line::Kept {
                tabs: kept
                    .iter()
                    .map(|kept| Tab {
                        path: kept.path.to_string_lossy().into_owned(),
                        name: kept.name.clone(),
                    })
                    .collect(),
            },
            Err(reason) => Line::Unavailable { reason },
        })
    })
    .await
}

#[tauri::command]
pub async fn open_line(app: AppHandle, store: State<'_, Store>) -> Result<Opened, String> {
    match line().await? {
        Line::Kept { tabs } => open_all(&app, &store, tabs, Some(0)).await,
        Line::Unavailable { reason } => Err(reason),
    }
}

#[tauri::command]
pub async fn sidebar(path: String) -> Result<Sidebar, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let text = |e: furca_core::Error| e.to_string();
        Ok(Sidebar {
            head: repo.head().map_err(text)?,
            refs: repo.refs().map_err(text)?,
            remotes: repo.remotes().map_err(text)?,
            stashes: repo.stashes().map_err(text)?,
            submodules: repo.submodules().map_err(text)?,
        })
    })
    .await
}

/// Starts the background work on an open repository - the window asks once
/// it has drawn, so the work never competes with the first frame.
#[tauri::command]
pub async fn maintain(app: AppHandle, path: String) -> Result<(), String> {
    maintenance::commit_graph(&app, &path);
    Ok(())
}

#[tauri::command]
pub async fn standing(path: String) -> Result<Standing, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        repo.standing().map_err(|e| e.to_string())
    })
    .await
}
