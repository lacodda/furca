//! A commit's details and one file's change, for the pane under the graph.
//!
//! Both are read on a thread of their own: a commit of a few thousand files
//! reads every blob it touched to count their lines, and a long file's diff
//! is a read of both sides - neither may hold up the window.

use furca_core::{Details, FileChange, FileDiff, Repository};

use crate::commands::off_thread;

/// The commit `id` of the repository at `path`: who made it, the whole
/// message and the files it changed against its first parent.
#[tauri::command]
pub async fn details(path: String, id: String) -> Result<Details, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.details(&id))
            .map_err(|e| e.to_string())
    })
    .await
}

/// One file's change, as `details` listed it: both texts and the hunks
/// between them.
#[tauri::command]
pub async fn file_diff(path: String, file: FileChange) -> Result<FileDiff, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.diff(&file))
            .map_err(|e| e.to_string())
    })
    .await
}
