//! Fetching, pulling and pushing from the window, and the remotes themselves.
//!
//! Each runs the system `git` through `furca-core` (ADR 0021), off the thread
//! that draws, and tells the window how far it has got down the channel it was
//! given. One run that talks to a remote goes at a time: a second click waits
//! for the first rather than meeting it at the refs. A fetch only moves
//! remote-tracking branches, so staging goes on beside it; a pull changes the
//! working tree and the index, so it queues with staging too.
//!
//! What a release pushes and tags is the release's: while one runs here, a
//! push, a pull or a change to the remotes is refused. A fetch is not - it
//! moves nothing the release reads.

use std::sync::Mutex;

use furca_core::{Fetched, Progress, Pulled, Pushed, Reconcile, Remotes, Repository};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};

use crate::commands::off_thread;
use crate::commit::changing;
use crate::working::{REFS_MOVED, RefsMoved};

/// Held for the length of each run that talks to a remote, or changes the
/// list of them.
static REACHING: Mutex<()> = Mutex::new(());

/// What the window hears of a run that talks to a remote, as it goes.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Reaching {
    /// The remote being reached, when they are reached one at a time; `None`
    /// for a fetch of every remote, which git goes through itself.
    pub remote: Option<String>,
    pub progress: Progress,
}

/// What a push did, remote by remote.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct Pushes {
    /// Each remote the branch reached, in the order it was pushed.
    pub pushed: Vec<Pushed>,
    /// Each remote it did not, and git's words for why.
    pub failed: Vec<NotPushed>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct NotPushed {
    pub remote: String,
    pub error: String,
}

/// Runs `work` on the repository at `path` off the thread that draws, one at
/// a time, refused while a release runs here unless `beside_a_release`, and
/// says that refs moved afterwards, whichever way it went.
async fn reaching<T: Send + 'static>(
    app: &AppHandle,
    path: String,
    beside_a_release: bool,
    work: impl FnOnce(&Repository) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let at = path.clone();
    let done = off_thread(move || {
        let _one_at_a_time = REACHING
            .lock()
            .map_err(|_| "a run on a remote went wrong")?;
        let repo = Repository::at(&at).map_err(|e| e.to_string())?;
        if !beside_a_release && furca_release::running(&repo) {
            return Err(
                "a release is running here: it pushes and tags as it goes, and waits for nothing"
                    .to_owned(),
            );
        }
        work(&repo)
    })
    .await;
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    done
}

/// Fetches from the remotes `from` names, taking away what the remote no
/// longer has when `prune` says so.
#[tauri::command]
pub async fn fetch(
    app: AppHandle,
    path: String,
    from: Remotes,
    prune: bool,
    heard: Channel<Reaching>,
) -> Result<Fetched, String> {
    reaching(&app, path, true, move |repo| {
        let remote = match &from {
            Remotes::All => None,
            Remotes::Default | Remotes::Named(_) => repo
                .fetch_remotes(&from)
                .ok()
                .and_then(|r| r.into_iter().next()),
        };
        repo.git()
            .fetch(&from, prune, &mut |progress| {
                let _ = heard.send(Reaching {
                    remote: remote.clone(),
                    progress: progress.clone(),
                });
            })
            .map_err(|e| e.to_string())
    })
    .await
}

/// Pulls into the current branch from the branch it follows, the way `how`
/// says.
#[tauri::command]
pub async fn pull(
    app: AppHandle,
    path: String,
    how: Reconcile,
    heard: Channel<Reaching>,
) -> Result<Pulled, String> {
    let pulled = changing(&app, path.clone(), move |repo| {
        let _one_at_a_time = REACHING
            .lock()
            .map_err(|_| furca_core::Error::Remote("a run on a remote went wrong".to_owned()))?;
        let remote = repo.upstream()?.map(|upstream| upstream.remote);
        repo.git().pull(how, &mut |progress| {
            let _ = heard.send(Reaching {
                remote: remote.clone(),
                progress: progress.clone(),
            });
        })
    })
    .await;
    // A pull that met a conflict failed, and still moved the working tree.
    let _ = app.emit(REFS_MOVED, RefsMoved { path });
    pulled
}

/// Pushes the current branch to the remotes `to` names, one after another,
/// with a lease when `force`. A remote that refuses does not stop the others.
#[tauri::command]
pub async fn push(
    app: AppHandle,
    path: String,
    to: Remotes,
    force: bool,
    heard: Channel<Reaching>,
) -> Result<Pushes, String> {
    reaching(&app, path, false, move |repo| {
        let destinations = repo.destinations(&to).map_err(|e| e.to_string())?;
        let mut pushes = Pushes {
            pushed: Vec::new(),
            failed: Vec::new(),
        };
        for destination in &destinations {
            let done = repo.git().push_branch(destination, force, &mut |progress| {
                let _ = heard.send(Reaching {
                    remote: Some(destination.remote.clone()),
                    progress: progress.clone(),
                });
            });
            match done {
                Ok(pushed) => pushes.pushed.push(pushed),
                Err(error) => pushes.failed.push(NotPushed {
                    remote: destination.remote.clone(),
                    error: error.to_string(),
                }),
            }
        }
        Ok(pushes)
    })
    .await
}

/// Adds the remote `name`, fetched from `url`. The window fetches it next.
#[tauri::command]
pub async fn remote_add(
    app: AppHandle,
    path: String,
    name: String,
    url: String,
) -> Result<(), String> {
    reaching(&app, path, false, move |repo| {
        repo.git()
            .remote_add(name.trim(), url.trim())
            .map_err(|e| e.to_string())
    })
    .await
}

/// Renames the remote `name` to `rename`, and points it at `url`; either one
/// alone when the other is as it was.
#[tauri::command]
pub async fn remote_edit(
    app: AppHandle,
    path: String,
    name: String,
    rename: String,
    url: String,
) -> Result<(), String> {
    reaching(&app, path, false, move |repo| {
        let (rename, url) = (rename.trim(), url.trim());
        let was = repo.remote_url(&name).map_err(|e| e.to_string())?;
        if was.as_deref() != Some(url) {
            repo.git()
                .remote_set_url(&name, url)
                .map_err(|e| e.to_string())?;
        }
        if rename != name {
            repo.git()
                .remote_rename(&name, rename)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
}

/// Removes the remote `name` and its remote-tracking branches. The window has
/// asked first.
#[tauri::command]
pub async fn remote_remove(app: AppHandle, path: String, name: String) -> Result<(), String> {
    reaching(&app, path, false, move |repo| {
        repo.git().remote_remove(&name).map_err(|e| e.to_string())
    })
    .await
}

/// Takes away the remote-tracking branches of `name` whose branch the remote
/// no longer has.
#[tauri::command]
pub async fn remote_prune(app: AppHandle, path: String, name: String) -> Result<Fetched, String> {
    reaching(&app, path, false, move |repo| {
        repo.git().remote_prune(&name).map_err(|e| e.to_string())
    })
    .await
}
