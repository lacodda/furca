//! Staging and committing from the window, and the console that shows what it
//! had git do.
//!
//! Every change here runs the system `git` through `furca-core` (ADR 0001), off
//! the thread that draws, and one at a time: two quick clicks queue up rather
//! than meet at `index.lock`. A release going in another door holds the index
//! for itself until it ends, because whatever is staged goes into its commit.
//!
//! The window stays honest about what it did by reading the list again after
//! each change rather than guessing how the change moved it.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;

use furca_core::{FileChange, GitRun, Picked, Repository};
use furca_release::message::{self, Finding, Parts};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::off_thread;
use crate::state;
use crate::working::{REFS_MOVED, RefsMoved, Watched};

/// The event a run of git is heard on.
pub const CONSOLE: &str = "console";

/// How many runs of each repository the console keeps.
const KEPT: usize = 500;

/// What a window hears of a run of git.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ConsoleLine {
    /// The repository, as its tab knows it.
    pub path: String,
    pub run: GitRun,
}

/// The runs of git each repository had this window make, oldest first. In
/// memory only: a console is what this window did, and a restart starts one
/// clean.
#[derive(Default)]
pub struct Console(Mutex<HashMap<String, VecDeque<GitRun>>>);

impl Console {
    fn keep(&self, path: &str, run: &GitRun) {
        if let Ok(mut all) = self.0.lock() {
            let runs = all.entry(path.to_owned()).or_default();
            runs.push_back(run.clone());
            while runs.len() > KEPT {
                runs.pop_front();
            }
        }
    }
}

/// Has every run of git in this process heard by the console, the engine's
/// pushes and the commit-graph's writes with the window's own commits.
pub fn listen(app: &AppHandle) {
    let app = app.clone();
    furca_core::hear_git_runs(move |dir, run| {
        let path = key(dir);
        app.state::<Console>().keep(&path, run);
        let _ = app.emit(
            CONSOLE,
            ConsoleLine {
                path,
                run: run.clone(),
            },
        );
    });
}

/// A repository's directory as its tab knows it.
fn key(dir: &Path) -> String {
    state::normalize(dir).map_or_else(
        |_| dir.to_string_lossy().into_owned(),
        |path| path.to_string_lossy().into_owned(),
    )
}

/// What the console of the repository at `path` has so far.
#[tauri::command]
pub async fn console_log(console: State<'_, Console>, path: String) -> Result<Vec<GitRun>, String> {
    let all = console
        .0
        .lock()
        .map_err(|_| "the console is poisoned".to_owned())?;
    Ok(all
        .get(&path)
        .map(|runs| runs.iter().cloned().collect())
        .unwrap_or_default())
}

/// Held for the length of each change, which is what makes them queue up.
static CHANGING: Mutex<()> = Mutex::new(());

/// Runs `change` on the repository at `path`, off the thread that draws: one at
/// a time, never during a release, and the list is read again afterwards.
pub(crate) async fn changing<T: Send + 'static>(
    app: &AppHandle,
    path: String,
    change: impl FnOnce(&Repository) -> Result<T, furca_core::Error> + Send + 'static,
) -> Result<T, String> {
    changing_because(
        app,
        path,
        "a release is running here: whatever is staged goes into its commit",
        change,
    )
    .await
}

/// [`changing`], refused during a release with `refusal`: what the release
/// would lose to the change.
pub(crate) async fn changing_because<T: Send + 'static>(
    app: &AppHandle,
    path: String,
    refusal: &'static str,
    change: impl FnOnce(&Repository) -> Result<T, furca_core::Error> + Send + 'static,
) -> Result<T, String> {
    let at = path.clone();
    let done = off_thread(move || {
        let _one_at_a_time = CHANGING.lock().map_err(|_| "a change went wrong")?;
        let repo = Repository::at(&at).map_err(|e| e.to_string())?;
        if furca_release::running(&repo) {
            return Err(refusal.to_owned());
        }
        change(&repo).map_err(|e| e.to_string())
    })
    .await;
    app.state::<Watched>().read_again(&path);
    done
}

/// Stages exactly `paths`.
#[tauri::command]
pub async fn stage_paths(app: AppHandle, path: String, paths: Vec<String>) -> Result<(), String> {
    changing(&app, path, move |repo| {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        repo.git().add(&paths)
    })
    .await
}

/// Which of the lists of the working copy to stage whole. The window is given a
/// thousand rows of each, so this is said by name and not as a list of paths.
#[derive(Debug, Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum List {
    /// Every change to a file git tracks.
    Unstaged,
    /// Every file git does not track and does not ignore.
    Untracked,
    /// All of it.
    Everything,
}

/// Stages a whole list of the working copy.
#[tauri::command]
pub async fn stage_list(app: AppHandle, path: String, list: List) -> Result<(), String> {
    changing(&app, path, move |repo| match list {
        List::Unstaged => repo.git().add_tracked(),
        List::Everything => repo.git().add_all(),
        List::Untracked => {
            let status = repo.status()?;
            let paths: Vec<&str> = status.untracked.iter().map(|f| f.path.as_str()).collect();
            repo.git().add(&paths)
        }
    })
    .await
}

/// Takes exactly `paths` out of the index.
#[tauri::command]
pub async fn unstage_paths(app: AppHandle, path: String, paths: Vec<String>) -> Result<(), String> {
    changing(&app, path, move |repo| {
        let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
        repo.git().reset(&paths)
    })
    .await
}

/// Takes everything out of the index.
#[tauri::command]
pub async fn unstage_all(app: AppHandle, path: String) -> Result<(), String> {
    changing(&app, path, |repo| repo.git().reset_all()).await
}

/// Stages lines of a file that is not staged: whole hunks, or some of their
/// lines.
#[tauri::command]
pub async fn stage_lines(
    app: AppHandle,
    path: String,
    file: FileChange,
    picks: Vec<Picked>,
) -> Result<(), String> {
    changing(&app, path, move |repo| repo.stage_lines(&file, &picks)).await
}

/// Takes lines of a staged file out of the index.
#[tauri::command]
pub async fn unstage_lines(
    app: AppHandle,
    path: String,
    file: FileChange,
    picks: Vec<Picked>,
) -> Result<(), String> {
    changing(&app, path, move |repo| repo.unstage_lines(&file, &picks)).await
}

/// Puts lines of a file that is not staged back as the index has them. The
/// window has asked first: nothing keeps what they were.
#[tauri::command]
pub async fn discard_lines(
    app: AppHandle,
    path: String,
    file: FileChange,
    picks: Vec<Picked>,
) -> Result<(), String> {
    changing(&app, path, move |repo| repo.discard_lines(&file, &picks)).await
}

/// Puts the files `tracked` back as the index has them, and deletes the
/// untracked files `untracked`. The window has asked first.
#[tauri::command]
pub async fn discard_paths(
    app: AppHandle,
    path: String,
    tracked: Vec<String>,
    untracked: Vec<String>,
) -> Result<(), String> {
    changing(&app, path, move |repo| {
        let tracked: Vec<&str> = tracked.iter().map(String::as_str).collect();
        let untracked: Vec<&str> = untracked.iter().map(String::as_str).collect();
        repo.git().restore(&tracked)?;
        repo.git().clean(&untracked)
    })
    .await
}

/// Discards a whole list of the working copy, said by name as staging one is:
/// every change git tracks that is not staged, every untracked file, or both.
#[tauri::command]
pub async fn discard_list(app: AppHandle, path: String, list: List) -> Result<(), String> {
    changing(&app, path, move |repo| match list {
        List::Unstaged => repo.git().restore_all(),
        List::Untracked => repo.git().clean_all(),
        List::Everything => {
            repo.git().restore_all()?;
            repo.git().clean_all()
        }
    })
    .await
}

/// The types of the convention, for the composer's list.
#[tauri::command]
pub async fn commit_types() -> Result<Vec<&'static str>, String> {
    Ok(message::TYPES.to_vec())
}

/// What the composer shows of a message as it is typed.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct MessageCheck {
    /// What furca's own reading says.
    pub findings: Vec<Finding>,
    /// What the repository's `commit-msg` hook says of it, in its own words;
    /// `None` when it has nothing against it.
    pub refused: Option<String>,
}

/// Reads `message`, and asks the repository's own hook about it.
#[tauri::command]
pub async fn check_message(path: String, message: String) -> Result<MessageCheck, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        Ok(MessageCheck {
            findings: message::check(&message),
            refused: repo
                .git()
                .commit_msg_says(&message)
                .map_err(|e| e.to_string())?,
        })
    })
    .await
}

/// Commits what is staged, or puts it into the last commit with `amend`, and
/// returns the id of the commit made. The hooks run as in a terminal.
#[tauri::command]
pub async fn commit(
    app: AppHandle,
    path: String,
    message: String,
    amend: bool,
) -> Result<String, String> {
    let id = changing(&app, path.clone(), move |repo| {
        if amend {
            repo.git().amend(&message)
        } else {
            repo.git().commit(&message)
        }
    })
    .await?;
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    Ok(id)
}

/// The last commit, as the composer starts an amend from it.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct LastCommit {
    pub parts: Parts,
    /// It is on the remote-tracking branch already: amending it rewrites what
    /// has been pushed.
    pub pushed: bool,
}

/// What the composer starts an amend from.
#[tauri::command]
pub async fn last_commit(path: String) -> Result<LastCommit, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let id = repo
            .head()
            .map_err(|e| e.to_string())?
            .commit
            .ok_or("there is no commit to amend yet")?;
        let text = repo.message(&id).map_err(|e| e.to_string())?;
        let upstream = repo.standing().map_err(|e| e.to_string())?.upstream;
        Ok(LastCommit {
            parts: message::parts(&text),
            pushed: upstream.is_some_and(|upstream| upstream.ahead == Some(0)),
        })
    })
    .await
}

/// Whether the Claude Code CLI is here to draft a message; asked once, since it
/// starts a program. Without one the composer has no button for it.
#[tauri::command]
pub async fn draft_available() -> Result<bool, String> {
    off_thread(|| Ok(crate::draft::available())).await
}

/// A message for what is staged, from the Claude Code CLI on this machine.
#[tauri::command]
pub async fn draft_message(path: String) -> Result<Parts, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let root = repo
            .workdir()
            .ok_or("a bare repository has nothing to commit")?
            .to_path_buf();
        let prompt = crate::draft::prompt(&repo).map_err(|e| e.to_string())?;
        crate::draft::ask(&root, &prompt).map(|reply| message::parts(&reply))
    })
    .await
}
