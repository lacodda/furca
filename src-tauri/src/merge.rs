//! Merging, cherry-picking and reverting from the window, the operation they
//! leave halfway, and resolving what it left in conflict.
//!
//! What each means is the core's (ADR 0023); this module runs it off the
//! thread that draws. A step goes one at a time with staging - it moves the
//! working tree and the index - and none goes while a release runs, which
//! commits on the branch it started on. A step that moves `HEAD` says so, as
//! a checkout does; a resolution only writes the index and a file, which the
//! watcher hears.

use furca_core::{Conflict, FastForward, Halfway, Proceed, Repository, Resolution, Stepped};
use tauri::AppHandle;

use crate::commands::off_thread;
use crate::commit::changing_because;
use crate::refs::naming;

/// Why a resolution waits for a release that runs.
const RELEASING: &str = "a release is running here: whatever is staged goes into its commit";

/// Merges `name` - a branch, a remote-tracking branch, a tag, a commit - into
/// the branch `HEAD` is on.
#[tauri::command]
pub async fn merge(
    app: AppHandle,
    path: String,
    name: String,
    ff: FastForward,
) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().merge(&name, ff)).await
}

/// Applies the change `commit` made onto `HEAD`, as a new commit.
#[tauri::command]
pub async fn cherry_pick(app: AppHandle, path: String, commit: String) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().cherry_pick(&[&commit])).await
}

/// Undoes the change `commit` made, with a new commit.
#[tauri::command]
pub async fn revert(app: AppHandle, path: String, commit: String) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().revert(&[&commit])).await
}

/// Continues, skips or aborts the operation halfway.
#[tauri::command]
pub async fn proceed(app: AppHandle, path: String, how: Proceed) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().proceed(how)).await
}

/// The operation halfway in the repository at `path`, if any.
#[tauri::command]
pub async fn halfway(path: String) -> Result<Option<Halfway>, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.halfway())
            .map_err(|e| e.to_string())
    })
    .await
}

/// The conflict at `file`: its three sides, and its lines in blocks.
#[tauri::command]
pub async fn conflict(path: String, file: String) -> Result<Conflict, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.conflict(&file))
            .map_err(|e| e.to_string())
    })
    .await
}

/// Resolves the conflict at `file` as `how` says, and stages it.
#[tauri::command]
pub async fn resolve(
    app: AppHandle,
    path: String,
    file: String,
    how: Resolution,
) -> Result<(), String> {
    changing_because(&app, path, RELEASING, move |repo| {
        repo.git().resolve(&file, &how)
    })
    .await
}
