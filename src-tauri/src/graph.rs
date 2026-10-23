//! The graph of each open repository, kept on this side of the window (ADR
//! 0012): a history can be a hundred thousand commits, and the webview only
//! ever draws the screenful it shows.
//!
//! A graph is begun when its tab asks for it and laid out a first screen's
//! worth at once; the rest is laid out on a thread of its own, a chunk at a
//! time, and the window hears after each chunk how far it has come (`graph`
//! events). The window then asks for the rows it draws. A graph is kept
//! until its tab closes, and begun again when the refs moved under it - a
//! commit, a fetch, a new branch - which the window finds out by asking for
//! it again.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use furca_core::{Commit, Graph, Query, Repository, Row, Tips};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

use crate::nooma;
use crate::state::WindowState;

/// The event the window hears a graph grow on.
pub const EVENT: &str = "graph";

/// Rows laid out before a graph is first answered with: a few screens.
const FIRST: usize = 500;
/// Rows laid out at a time after that. A chunk holds the graph for a few
/// milliseconds, which is as long as a read of rows waits behind it.
const CHUNK: usize = 10_000;
/// Rows searched at a time; between two, a newer search takes over.
const SEARCHED: usize = 4_096;
/// The most rows one ask returns.
const MOST_ROWS: usize = 1_000;

/// Every graph the window holds, by the path of its repository.
#[derive(Default)]
pub struct Graphs {
    open: Mutex<HashMap<String, Arc<Entry>>>,
    /// The graph of the tab in front, begun while the webview starts.
    early: Mutex<Option<(String, Beginning)>>,
}

/// A graph being begun on a thread of its own.
type Beginning = JoinHandle<Result<Arc<Entry>, String>>;

pub(crate) struct Entry {
    generation: u64,
    branch: Option<String>,
    graph: Mutex<Graph>,
    /// The newest search's ticket: an older search that sees a newer one
    /// stops.
    search: AtomicU64,
}

/// Counts graphs, so one begun again is told apart from the one before it.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// How far a graph is laid out.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct GraphState {
    /// Which graph of the repository this is. A graph begun again has a new
    /// one, and rows asked of an older one are refused rather than answered
    /// from a history that has moved.
    pub generation: u64,
    /// The branch it is the history of, as a full ref name; `null` for every
    /// branch and tag.
    pub branch: Option<String>,
    /// How many rows are laid out.
    pub rows: usize,
    /// The most lanes a row needs side by side.
    pub width: u16,
    /// Whether the history is laid out to its end.
    pub complete: bool,
}

/// A graph grew: what the window hears.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Grown {
    pub path: String,
    pub state: GraphState,
}

/// One row as the window draws it: its lines, and the commit on it.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct GraphRow {
    pub row: Row,
    pub commit: Commit,
}

/// What a search found, as it finds it.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Found {
    /// More rows `by` found, in order.
    Rows { by: Finder, rows: Vec<usize> },
    /// `by` has searched all it will.
    Done { by: Finder },
    /// `by` could not search, and why.
    Failed { by: Finder, reason: String },
}

/// Who searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum Finder {
    /// furca itself: the subject, the author, the start of the id.
    Graph,
    /// nooma's history of the repository, when nooma is installed: the
    /// whole message.
    Nooma,
}

impl Entry {
    fn begin(path: &str, branch: Option<String>) -> Result<Arc<Self>, String> {
        let repo = Repository::at(path).map_err(|e| e.to_string())?;
        let tips = match &branch {
            Some(branch) => Tips::Revisions(vec![branch.clone()]),
            None => Tips::All,
        };
        let mut graph = repo.graph(tips).map_err(|e| e.to_string())?;
        graph.extend(&repo, FIRST).map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            generation: GENERATION.fetch_add(1, Ordering::Relaxed),
            branch,
            graph: Mutex::new(graph),
            search: AtomicU64::new(0),
        }))
    }

    fn graph(&self) -> Result<MutexGuard<'_, Graph>, String> {
        self.graph
            .lock()
            .map_err(|_| "the graph is poisoned".to_owned())
    }

    fn state(&self, graph: &Graph) -> GraphState {
        GraphState {
            generation: self.generation,
            branch: self.branch.clone(),
            rows: graph.len(),
            width: graph.width(),
            complete: graph.is_complete(),
        }
    }
}

impl Graphs {
    /// Begins the graph of the tab in front of `state`, if any, on a thread
    /// of its own: the webview takes hundreds of milliseconds to come up, and
    /// the graph need not wait for it to ask.
    pub fn begin_early(&self, state: &WindowState) {
        let Some(tab) = state.active.and_then(|index| state.tabs.get(index)) else {
            return;
        };
        let path = tab.path.clone();
        let beginning = std::thread::spawn({
            let path = path.clone();
            move || Entry::begin(&path, None)
        });
        if let Ok(mut early) = self.early.lock() {
            *early = Some((path, beginning));
        }
    }

    /// The early graph of `path`, once, if it is the history of everything.
    fn take_early(&self, path: &str, branch: &Option<String>) -> Option<Beginning> {
        let mut early = self.early.lock().ok()?;
        match early.take() {
            Some((begun, beginning)) if begun == path && branch.is_none() => Some(beginning),
            other => {
                *early = other;
                None
            }
        }
    }

    fn get(&self, path: &str) -> Result<Option<Arc<Entry>>, String> {
        Ok(self
            .open
            .lock()
            .map_err(|_| "the graphs are poisoned")?
            .get(path)
            .cloned())
    }

    /// The graph of `path` the window asked about, as long as it is still the
    /// one the window has.
    fn current(&self, path: &str, generation: u64) -> Result<Arc<Entry>, String> {
        self.get(path)?
            .filter(|entry| entry.generation == generation)
            .ok_or_else(|| "the graph was begun again; ask for it afresh".to_owned())
    }
}

/// Lays out the rest of the graph on a thread of its own, a chunk at a time,
/// telling the window after each. Stops when the graph is closed or begun
/// again: the window then holds no reference to it.
fn grow(app: AppHandle, path: String, entry: Weak<Entry>) {
    std::thread::spawn(move || {
        let Ok(repo) = Repository::at(&path) else {
            return;
        };
        loop {
            let Some(entry) = entry.upgrade() else {
                return;
            };
            let state = {
                let Ok(mut graph) = entry.graph() else {
                    return;
                };
                if graph.is_complete() {
                    return;
                }
                if let Err(error) = graph.extend(&repo, CHUNK) {
                    eprintln!("furca: the graph of {path} stopped growing: {error}");
                    return;
                }
                entry.state(&graph)
            };
            let complete = state.complete;
            let _ = app.emit(
                EVENT,
                Grown {
                    path: path.clone(),
                    state,
                },
            );
            if complete {
                return;
            }
        }
    });
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

/// The graph of the repository at `path`, of `branch` (a full ref name) or
/// of everything: the one held, while it still shows what the refs lead to,
/// or one begun afresh.
#[tauri::command]
pub async fn graph(
    app: AppHandle,
    graphs: State<'_, Graphs>,
    path: String,
    branch: Option<String>,
) -> Result<GraphState, String> {
    if let Some(entry) = graphs.get(&path)?.filter(|entry| entry.branch == branch) {
        let held = entry.clone();
        let at = path.clone();
        let current = off_thread(move || {
            let repo = Repository::at(&at).map_err(|e| e.to_string())?;
            let graph = held.graph()?;
            let current = repo.graph_is_current(&graph).map_err(|e| e.to_string())?;
            Ok(current.then(|| held.state(&graph)))
        })
        .await?;
        if let Some(state) = current {
            return Ok(state);
        }
    }

    let beginning = graphs.take_early(&path, &branch);
    let entry = {
        let path = path.clone();
        off_thread(move || match beginning {
            Some(beginning) => beginning
                .join()
                .unwrap_or_else(|_| Err("the early graph stopped".to_owned())),
            None => Entry::begin(&path, branch),
        })
        .await?
    };
    graphs
        .open
        .lock()
        .map_err(|_| "the graphs are poisoned")?
        .insert(path.clone(), entry.clone());
    let state = entry.state(&*entry.graph()?);
    if !state.complete {
        grow(app, path, Arc::downgrade(&entry));
    }
    Ok(state)
}

/// The rows `from` to `from + count` of a graph, with their commits, as far
/// as they are laid out.
#[tauri::command]
pub async fn graph_rows(
    graphs: State<'_, Graphs>,
    path: String,
    generation: u64,
    from: usize,
    count: usize,
) -> Result<Vec<GraphRow>, String> {
    let entry = graphs.current(&path, generation)?;
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let graph = entry.graph()?;
        let range = from..from.saturating_add(count.min(MOST_ROWS));
        let rows = graph.rows(range.clone());
        let commits = graph.commits(&repo, range).map_err(|e| e.to_string())?;
        Ok(rows
            .into_iter()
            .zip(commits)
            .map(|(row, commit)| GraphRow { row, commit })
            .collect())
    })
    .await
}

/// The row of the commit `id` in a graph, laid out as far as it takes;
/// `null` when the graph does not hold it.
#[tauri::command]
pub async fn graph_position(
    graphs: State<'_, Graphs>,
    path: String,
    generation: u64,
    id: String,
) -> Result<Option<usize>, String> {
    let entry = graphs.current(&path, generation)?;
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let mut graph = entry.graph()?;
        loop {
            if let Some(at) = graph.position(&id) {
                return Ok(Some(at));
            }
            if graph.is_complete() {
                return Ok(None);
            }
            graph.extend(&repo, CHUNK).map_err(|e| e.to_string())?;
        }
    })
    .await
}

/// Searches a graph for `query` and sends what it finds to `found` as it
/// finds it; a newer search of the same graph stops this one. Answers at once
/// with who is searching: furca, and nooma when it is installed.
#[tauri::command]
pub async fn graph_find(
    graphs: State<'_, Graphs>,
    path: String,
    generation: u64,
    query: String,
    found: Channel<Found>,
) -> Result<Vec<Finder>, String> {
    let entry = graphs.current(&path, generation)?;
    let ticket = entry.search.fetch_add(1, Ordering::SeqCst) + 1;
    let Some(parsed) = Query::new(&query) else {
        return Ok(Vec::new());
    };

    let mut finders = vec![Finder::Graph];
    {
        let entry = entry.clone();
        let found = found.clone();
        let path = path.clone();
        std::thread::spawn(move || search_graph(&entry, ticket, &path, &parsed, &found));
    }
    if let Some(nooma) = nooma::installed() {
        finders.push(Finder::Nooma);
        std::thread::spawn(move || search_nooma(&entry, ticket, nooma, &path, &query, &found));
    }
    Ok(finders)
}

fn superseded(entry: &Entry, ticket: u64) -> bool {
    entry.search.load(Ordering::SeqCst) != ticket
}

fn search_graph(entry: &Entry, ticket: u64, path: &str, query: &Query, found: &Channel<Found>) {
    let by = Finder::Graph;
    let failed = |reason: String| {
        let _ = found.send(Found::Failed { by, reason });
    };
    let repo = match Repository::at(path) {
        Ok(repo) => repo,
        Err(error) => return failed(error.to_string()),
    };
    let mut from = 0;
    loop {
        if superseded(entry, ticket) {
            return;
        }
        let (rows, laid_out, complete) = {
            let Ok(graph) = entry.graph() else {
                return failed("the graph is poisoned".to_owned());
            };
            let end = (from + SEARCHED).min(graph.len());
            match graph.find(&repo, query, from..end) {
                Ok(rows) => (rows, end, graph.is_complete() && end == graph.len()),
                Err(error) => return failed(error.to_string()),
            }
        };
        if !rows.is_empty() && found.send(Found::Rows { by, rows }).is_err() {
            return;
        }
        if complete {
            let _ = found.send(Found::Done { by });
            return;
        }
        if laid_out == from {
            // Searched as far as the graph is laid out: wait for more.
            std::thread::sleep(Duration::from_millis(10));
        }
        from = laid_out;
    }
}

fn search_nooma(
    entry: &Entry,
    ticket: u64,
    nooma: &std::path::Path,
    path: &str,
    query: &str,
    found: &Channel<Found>,
) {
    let by = Finder::Nooma;
    let ids = match nooma::history(nooma, path, query) {
        Ok(ids) => ids,
        Err(reason) => {
            let _ = found.send(Found::Failed { by, reason });
            return;
        }
    };
    // nooma answers by commit; the window wants rows, and a commit low in a
    // large history has a row only once the graph is laid out that far.
    loop {
        if superseded(entry, ticket) {
            return;
        }
        let Ok(graph) = entry.graph() else {
            return;
        };
        if graph.is_complete() {
            let rows = graph.positions(&ids);
            drop(graph);
            if !rows.is_empty() {
                let _ = found.send(Found::Rows { by, rows });
            }
            let _ = found.send(Found::Done { by });
            return;
        }
        drop(graph);
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Lets go of the graph of `path`: its tab closed.
#[tauri::command]
pub async fn forget_graph(graphs: State<'_, Graphs>, path: String) -> Result<(), String> {
    graphs
        .open
        .lock()
        .map_err(|_| "the graphs are poisoned")?
        .remove(&path);
    Ok(())
}
