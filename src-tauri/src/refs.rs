//! Branches and tags from the window: checking out, making, renaming,
//! deleting, what a branch follows, and pushing a tag or taking it away.
//!
//! What each means is the core's (ADR 0022); this module only runs it off the
//! thread that draws and says afterwards that refs moved. A change here goes
//! one at a time with staging - a checkout moves the working tree and the
//! index - and none goes while a release runs: it commits, pushes and tags on
//! the branch it started on. A push of a tag, or taking one away from a
//! remote, goes one at a time with the other runs on a remote.

use furca_core::{Deleted, Remotes, Repository, Switched, TagPushed};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

use crate::commands::off_thread;
use crate::commit::changing_because;
use crate::remote::{NotPushed, Reaching, reaching};
use crate::working::{REFS_MOVED, RefsMoved};

/// Why a change to a branch or a tag waits for a release that runs.
const RELEASING: &str =
    "a release is running here: it commits, pushes and tags on the branch it started on";

/// Runs `change` as staging runs one, and says that refs moved afterwards,
/// whichever way it went.
async fn naming<T: Send + 'static>(
    app: &AppHandle,
    path: String,
    change: impl FnOnce(&Repository) -> Result<T, furca_core::Error> + Send + 'static,
) -> Result<T, String> {
    let done = changing_because(app, path.clone(), RELEASING, change).await;
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    done
}

/// Checks out the branch `name`: a local one, or a remote-tracking one as
/// the local branch that follows it.
#[tauri::command]
pub async fn checkout(app: AppHandle, path: String, name: String) -> Result<Switched, String> {
    naming(&app, path, move |repo| {
        let to = repo.checkout_of(&name)?;
        repo.git().checkout(&to)
    })
    .await
}

/// Checks out the commit `revision` names on no branch.
#[tauri::command]
pub async fn checkout_detached(
    app: AppHandle,
    path: String,
    revision: String,
) -> Result<Switched, String> {
    naming(&app, path, move |repo| {
        let to = repo.checkout_detached(&revision)?;
        repo.git().checkout(&to)
    })
    .await
}

/// Makes the branch `name` at `from`, and checks it out when `checkout`.
#[tauri::command]
pub async fn branch_create(
    app: AppHandle,
    path: String,
    name: String,
    from: String,
    checkout: bool,
) -> Result<(), String> {
    naming(&app, path, move |repo| {
        let name = name.trim();
        if checkout {
            let to = repo.checkout_new(name, &from)?;
            repo.git().checkout(&to).map(drop)
        } else {
            repo.git().branch_create(name, &from)
        }
    })
    .await
}

/// Renames the branch `from` to `to`.
#[tauri::command]
pub async fn branch_rename(
    app: AppHandle,
    path: String,
    from: String,
    to: String,
) -> Result<(), String> {
    naming(&app, path, move |repo| {
        repo.git().branch_rename(&from, to.trim())
    })
    .await
}

/// How many commits only the branch `name` holds: what the window says
/// before it is deleted.
#[tauri::command]
pub async fn branch_lost(path: String, name: String) -> Result<usize, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.only_on(&name))
            .map_err(|e| e.to_string())
    })
    .await
}

/// Deletes the branch `name`, which the window said holds `lost` commits
/// nothing else has. One that holds more by now is refused: the answer was
/// given to a smaller question.
#[tauri::command]
pub async fn branch_delete(
    app: AppHandle,
    path: String,
    name: String,
    lost: usize,
) -> Result<Deleted, String> {
    naming(&app, path, move |repo| {
        let now = repo.only_on(&name)?;
        if now > lost {
            return Err(furca_core::Error::Unmerged {
                branch: name,
                commits: now,
            });
        }
        repo.git().branch_delete(&name, true)
    })
    .await
}

/// Has the branch `name` follow `upstream`, or nothing.
#[tauri::command]
pub async fn branch_follow(
    app: AppHandle,
    path: String,
    name: String,
    upstream: Option<String>,
) -> Result<(), String> {
    naming(&app, path, move |repo| {
        repo.git().branch_follow(&name, upstream.as_deref())
    })
    .await
}

/// Tags `target` as `name`: annotated with `message` when there is one.
#[tauri::command]
pub async fn tag_create(
    app: AppHandle,
    path: String,
    name: String,
    target: String,
    message: Option<String>,
) -> Result<(), String> {
    naming(&app, path, move |repo| {
        repo.git().tag(name.trim(), &target, message.as_deref())
    })
    .await
}

/// What a push of a tag did, remote by remote.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct TagPushes {
    pub pushed: Vec<TagPushed>,
    pub failed: Vec<NotPushed>,
}

/// Pushes the tag `name` to the remotes `to` names, one after another. A
/// remote that refuses does not stop the others.
#[tauri::command]
pub async fn tag_push(
    app: AppHandle,
    path: String,
    name: String,
    to: Remotes,
    heard: Channel<Reaching>,
) -> Result<TagPushes, String> {
    reaching(&app, path, false, move |repo| {
        let remotes = repo.push_remotes(&to).map_err(|e| e.to_string())?;
        let mut pushes = TagPushes {
            pushed: Vec::new(),
            failed: Vec::new(),
        };
        for remote in &remotes {
            let done = repo.git().push_tag(remote, &name, &mut |progress| {
                let _ = heard.send(Reaching {
                    remote: Some(remote.clone()),
                    progress: progress.clone(),
                });
            });
            match done {
                Ok(pushed) => pushes.pushed.push(pushed),
                Err(error) => pushes.failed.push(NotPushed {
                    remote: remote.clone(),
                    error: error.to_string(),
                }),
            }
        }
        Ok(pushes)
    })
    .await
}

/// What deleting a tag did: here, and on each remote it was taken from.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct TagDeletes {
    /// The tag as it was here; `None` when only the remotes had it.
    pub deleted: Option<Deleted>,
    pub remotes: Vec<TagPushed>,
    pub failed: Vec<NotPushed>,
}

/// Deletes the tag `name` here, and takes it away from each of `remotes`
/// first. The window has asked.
#[tauri::command]
pub async fn tag_delete(
    app: AppHandle,
    path: String,
    name: String,
    remotes: Vec<String>,
    heard: Channel<Reaching>,
) -> Result<TagDeletes, String> {
    reaching(&app, path, false, move |repo| {
        let mut deletes = TagDeletes {
            deleted: None,
            remotes: Vec::new(),
            failed: Vec::new(),
        };
        for remote in &remotes {
            let done = repo.git().push_tag_delete(remote, &name, &mut |progress| {
                let _ = heard.send(Reaching {
                    remote: Some(remote.clone()),
                    progress: progress.clone(),
                });
            });
            match done {
                Ok(gone) => deletes.remotes.push(gone),
                Err(error) => deletes.failed.push(NotPushed {
                    remote: remote.clone(),
                    error: error.to_string(),
                }),
            }
        }
        let here = repo
            .refs()
            .map_err(|e| e.to_string())?
            .tags
            .iter()
            .any(|tag| tag.name == name);
        if here || remotes.is_empty() {
            deletes.deleted = Some(repo.git().tag_delete(&name).map_err(|e| e.to_string())?);
        }
        Ok(deletes)
    })
    .await
}
