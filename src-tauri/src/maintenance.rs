//! Work the window does on a repository's behalf, in the background: for now,
//! having git write the commit-graph when it is missing or far behind
//! (ADR 0010).

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use furca_core::Repository;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The event the window hears the work's progress on.
pub const EVENT: &str = "maintenance";

/// One step of background work on a repository.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Maintenance {
    /// The repository, as its tab knows it.
    pub path: String,
    pub step: Step,
}

/// Where the work is.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Step {
    /// git is writing the commit-graph.
    Writing,
    /// It is written, in `millis`.
    Written { millis: u64 },
    /// It could not be, and git's reason; nothing else is affected.
    Failed { reason: String },
}

/// The repositories being worked on, so one is never worked on twice at once.
static RUNNING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

/// Checks the commit-graph of the repository at `path` and, if it is worth
/// writing, has git write it, on a thread of its own. A repository already
/// being worked on is left to that work.
pub fn commit_graph(app: &AppHandle, path: &str) {
    let fresh = RUNNING
        .lock()
        .map(|mut running| running.insert(path.to_owned()))
        .unwrap_or(false);
    if !fresh {
        return;
    }
    let app = app.clone();
    let path = path.to_owned();
    std::thread::spawn(move || {
        let emit = |step: Step| {
            let _ = app.emit(
                EVENT,
                Maintenance {
                    path: path.clone(),
                    step,
                },
            );
        };
        match write_if_behind(&path, || emit(Step::Writing)) {
            Ok(Some(millis)) => emit(Step::Written { millis }),
            Ok(None) => {}
            Err(reason) => emit(Step::Failed { reason }),
        }
        if let Ok(mut running) = RUNNING.lock() {
            running.remove(&path);
        }
    });
}

/// Writes the commit-graph if it is worth it, calling `starting` first, and
/// says how long git took; `None` when it was not worth writing.
fn write_if_behind(path: &str, starting: impl FnOnce()) -> Result<Option<u64>, String> {
    let repo = Repository::at(path).map_err(|e| e.to_string())?;
    if !repo
        .commit_graph()
        .map_err(|e| e.to_string())?
        .worth_writing()
    {
        return Ok(None);
    }
    starting();
    let started = Instant::now();
    repo.git().write_commit_graph().map_err(|e| e.to_string())?;
    Ok(Some(started.elapsed().as_millis() as u64))
}
