//! Rebasing, squashing and absorbing from the window.
//!
//! What each means is the core's (ADR 0024); this module runs it off the
//! thread that draws, one at a time with staging - each moves the working
//! tree and the index - and never while a release runs. A step that moves
//! `HEAD` says so, as a merge does.

use std::collections::HashSet;

use furca_core::{Commit, FileChange, Picked, RebasePlan, RebaseTodo, Repository, Stepped};
use furca_release::message::{self, Parts};
use serde::Serialize;
use tauri::AppHandle;

use crate::commands::off_thread;
use crate::refs::naming;

/// How many commits the window offers to absorb a change into.
const TARGETS: usize = 200;

/// Replays the commits `HEAD` has and `onto` does not on `onto`.
#[tauri::command]
pub async fn rebase(app: AppHandle, path: String, onto: String) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().rebase(&onto)).await
}

/// The commits a rebase onto `onto` would replay, oldest first; with no
/// `onto`, every commit from the first.
#[tauri::command]
pub async fn rebase_plan(path: String, onto: Option<String>) -> Result<RebasePlan, String> {
    off_thread(move || {
        Repository::at(&path)
            .and_then(|repo| repo.rebase_plan(onto.as_deref()))
            .map_err(|e| e.to_string())
    })
    .await
}

/// Replays the commits of the plan for `onto` as `todo` says.
#[tauri::command]
pub async fn rebase_interactive(
    app: AppHandle,
    path: String,
    onto: Option<String>,
    todo: Vec<RebaseTodo>,
) -> Result<Stepped, String> {
    naming(&app, path, move |repo| {
        repo.git().rebase_interactive(onto.as_deref(), &todo)
    })
    .await
}

/// Makes one commit of `commit` and every commit after it, with `message`,
/// or with their messages one after another.
#[tauri::command]
pub async fn squash(
    app: AppHandle,
    path: String,
    commit: String,
    message: Option<String>,
) -> Result<Stepped, String> {
    naming(&app, path, move |repo| {
        repo.git().squash(&commit, message.as_deref())
    })
    .await
}

/// Puts what is staged into `commit`.
#[tauri::command]
pub async fn absorb(app: AppHandle, path: String, commit: String) -> Result<Stepped, String> {
    naming(&app, path, move |repo| repo.git().absorb(&commit)).await
}

/// Puts lines of a file that is not staged into `commit`: whole hunks, or
/// some of their lines.
#[tauri::command]
pub async fn absorb_lines(
    app: AppHandle,
    path: String,
    file: FileChange,
    picks: Vec<Picked>,
    commit: String,
) -> Result<Stepped, String> {
    naming(&app, path, move |repo| {
        repo.git().absorb_lines(&file, &picks, &commit)
    })
    .await
}

/// A commit a change can be absorbed into.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct AbsorbTarget {
    pub commit: Commit,
    /// The branch's upstream has it already: rewriting it parts the two.
    pub pushed: bool,
    /// It changed the file the change is in: the likeliest home for it.
    pub touches: bool,
}

/// The commits a change can be absorbed into, newest first: the line `HEAD`
/// came by, down to its first merge. With `file`, each says whether it
/// changed that file.
#[tauri::command]
pub async fn absorb_targets(
    path: String,
    file: Option<String>,
) -> Result<Vec<AbsorbTarget>, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let steps = repo.rewritable(TARGETS).map_err(|e| e.to_string())?;
        let touching: HashSet<String> = match &file {
            Some(file) => repo
                .file_log(file, TARGETS)
                .map_err(|e| e.to_string())?
                .commits
                .into_iter()
                .map(|revision| revision.commit.id)
                .collect(),
            None => HashSet::new(),
        };
        Ok(steps
            .into_iter()
            .map(|step| AbsorbTarget {
                touches: touching.contains(&step.commit.id),
                pushed: step.pushed,
                commit: step.commit,
            })
            .collect())
    })
    .await
}

/// The message git prepared for the commit that finishes the operation
/// halfway, as the commit form takes it; `None` when nothing is halfway, or
/// git wrote none.
#[tauri::command]
pub async fn prepared_message(path: String) -> Result<Option<Parts>, String> {
    off_thread(move || {
        let repo = Repository::at(&path).map_err(|e| e.to_string())?;
        let halfway = repo.halfway().map_err(|e| e.to_string())?;
        Ok(halfway
            .and_then(|halfway| halfway.text)
            .map(|text| message::parts(&text)))
    })
    .await
}
