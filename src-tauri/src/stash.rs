//! Putting changes away in the stash and getting them back, from the window.
//!
//! Each runs the system `git` through `furca-core` as staging does - off the
//! thread that draws, one change at a time, never during a release - and then
//! says that a ref moved: the stash is `refs/stash`, and the left column, the
//! graph and the standing all read it.

use furca_core::Stash;
use tauri::{AppHandle, Emitter};

use crate::commit::changing;
use crate::working::{REFS_MOVED, RefsMoved};

/// Puts every change away - staged, not staged and untracked - with `message`
/// when one is given. The entry made; `None` when there was nothing to put
/// away.
#[tauri::command]
pub async fn stash_push(
    app: AppHandle,
    path: String,
    message: Option<String>,
) -> Result<Option<Stash>, String> {
    let made = changing(&app, path.clone(), move |repo| {
        repo.git().stash(message.as_deref())
    })
    .await?;
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    Ok(made)
}

/// What to do with a stash entry.
#[derive(Debug, Clone, Copy, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "kebab-case")]
pub enum StashAction {
    /// Apply it and keep it.
    Apply,
    /// Apply it and drop it, unless it met a conflict.
    Pop,
    /// Drop it without applying it. The window has asked first.
    Drop,
}

/// Applies, pops or drops `stash`, as long as it is the same entry still.
#[tauri::command]
pub async fn stash_act(
    app: AppHandle,
    path: String,
    stash: Stash,
    action: StashAction,
) -> Result<(), String> {
    let done = changing(&app, path.clone(), move |repo| match action {
        StashAction::Apply => repo.git().stash_apply(&stash),
        StashAction::Pop => repo.git().stash_pop(&stash),
        StashAction::Drop => repo.git().stash_drop(&stash),
    })
    .await;
    // A pop that met a conflict failed, and still changed the working tree.
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    done
}
