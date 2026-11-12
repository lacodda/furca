//! The commands that make, move and take away names in the history - `branch`,
//! `checkout` and `tag`. Each is one call into the core, which decides what a
//! checkout means, which new branch follows what and what a deletion loses,
//! and has the system `git` do it (ADR 0022).
//!
//! A change prints a sentence for a person; for `--json`, the branch or tag
//! as it now is, read afresh - a handle reads the configuration once, when it
//! is opened, and what a branch follows is configuration.

use furca_core::{Branch, Deleted, Remotes, Repository, Switched, Tag, TagPushed};
use serde::Serialize;

use crate::reach::{NotPushed, Ticker};
use crate::{Answer, BranchCommand, Common, TagCommand, emit, line, open, say, short};

pub(crate) fn branch(command: Option<BranchCommand>, common: &Common) -> Result<Answer, String> {
    let Some(command) = command else {
        let branches = open(&common.repo)?
            .refs()
            .map_err(|e| e.to_string())?
            .branches;
        return emit(common.json, branches.as_slice(), branches_text);
    };
    let (common, name, said) = match command {
        BranchCommand::Create {
            common,
            name,
            from,
            checkout,
        } => {
            let repo = open(&common.repo)?;
            let said = if checkout {
                let to = repo.checkout_new(&name, &from).map_err(|e| e.to_string())?;
                let switched = repo.git().checkout(&to).map_err(|e| e.to_string())?;
                switched_text(&switched)
            } else {
                repo.git()
                    .branch_create(&name, &from)
                    .map_err(|e| e.to_string())?;
                let at = repo.commit(&from).map_err(|e| e.to_string())?.id;
                format!("Made {name} at {}.", short(&at))
            };
            (common, name, said)
        }
        BranchCommand::Rename { common, from, to } => {
            open(&common.repo)?
                .git()
                .branch_rename(&from, &to)
                .map_err(|e| e.to_string())?;
            let said = format!("Renamed {from} to {to}.");
            (common, to, said)
        }
        BranchCommand::Delete {
            common,
            name,
            force,
        } => {
            let deleted = open(&common.repo)?
                .git()
                .branch_delete(&name, force)
                .map_err(|e| e.to_string())?;
            return emit(common.json, &deleted, deleted_branch_text);
        }
        BranchCommand::Follow {
            common,
            name,
            upstream,
        } => {
            open(&common.repo)?
                .git()
                .branch_follow(&name, Some(&upstream))
                .map_err(|e| e.to_string())?;
            let said = format!("{name} follows {upstream} now.");
            (common, name, said)
        }
        BranchCommand::Unfollow { common, name } => {
            open(&common.repo)?
                .git()
                .branch_follow(&name, None)
                .map_err(|e| e.to_string())?;
            let said = format!("{name} follows nothing now.");
            (common, name, said)
        }
    };
    if !common.json {
        println!("{said}");
        return Ok(Answer::Yes);
    }
    let branch = branch_named(&open(&common.repo)?, &name)?;
    emit(true, &branch, |_| String::new())
}

pub(crate) fn checkout(
    common: &Common,
    branch: Option<String>,
    detach: Option<String>,
) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let to = match (branch, detach) {
        (_, Some(revision)) => repo.checkout_detached(&revision),
        (Some(name), None) => repo.checkout_of(&name),
        (None, None) => return Err("name the branch to check out, or --detach a commit".into()),
    }
    .map_err(|e| e.to_string())?;
    let switched = repo.git().checkout(&to).map_err(|e| e.to_string())?;
    emit(common.json, &switched, switched_text)
}

/// What `tag delete` did, for `--json`.
#[derive(Serialize)]
struct TagDeleted {
    /// The tag as it was here; `null` when only the remotes had it.
    deleted: Option<Deleted>,
    /// Each remote it was taken away from.
    remotes: Vec<TagPushed>,
    /// Each remote it was not, and git's words for why.
    failed: Vec<NotPushed>,
}

/// What `tag push` did, for `--json`.
#[derive(Serialize)]
struct TagPushes {
    tag: String,
    pushed: Vec<TagPushed>,
    failed: Vec<NotPushed>,
}

pub(crate) fn tag(command: Option<TagCommand>, common: &Common) -> Result<Answer, String> {
    let Some(command) = command else {
        let tags = open(&common.repo)?.refs().map_err(|e| e.to_string())?.tags;
        return emit(common.json, tags.as_slice(), tags_text);
    };
    match command {
        TagCommand::Create {
            common,
            name,
            target,
            message,
        } => {
            open(&common.repo)?
                .git()
                .tag(&name, &target, message.as_deref())
                .map_err(|e| e.to_string())?;
            let tag = open(&common.repo)?
                .refs()
                .map_err(|e| e.to_string())?
                .tags
                .into_iter()
                .find(|tag| tag.name == name)
                .ok_or_else(|| format!("git made {name}, and it is not here"))?;
            emit(common.json, &tag, |tag| {
                format!("Tagged {} {}.", short(&tag.target), tag_words(tag))
            })
        }
        TagCommand::Delete {
            common,
            name,
            remote,
            all_remotes,
        } => {
            let repo = open(&common.repo)?;
            let from = match (remote, all_remotes) {
                (Some(remote), _) => repo.push_remotes(&Remotes::Named(remote)),
                (None, true) => repo.push_remotes(&Remotes::All),
                (None, false) => Ok(Vec::new()),
            }
            .map_err(|e| e.to_string())?;
            let mut remotes = Vec::new();
            let mut failed = Vec::new();
            let mut ticker = Ticker::new("tag");
            for remote in &from {
                let done = repo
                    .git()
                    .push_tag_delete(remote, &name, &mut |progress| ticker.hear(remote, progress));
                ticker.clear();
                match done {
                    Ok(gone) => remotes.push(gone),
                    Err(error) => {
                        say("tag", &format!("not taken away from {remote}"));
                        failed.push(not_pushed(remote, &name, &error));
                    }
                }
            }
            // A tag only a remote has is taken away from there, and from here
            // there is nothing to take.
            let here = repo
                .refs()
                .map_err(|e| e.to_string())?
                .tags
                .iter()
                .any(|tag| tag.name == name);
            let deleted = if here || from.is_empty() {
                Some(repo.git().tag_delete(&name).map_err(|e| e.to_string())?)
            } else {
                None
            };
            let answer = TagDeleted {
                deleted,
                remotes,
                failed,
            };
            emit(common.json, &answer, tag_deleted_text)?;
            refused(&answer.failed)
        }
        TagCommand::Push {
            common,
            name,
            remote,
            all_remotes,
        } => {
            let repo = open(&common.repo)?;
            let to = match (remote, all_remotes) {
                (Some(name), _) => Remotes::Named(name),
                (None, true) => Remotes::All,
                (None, false) => Remotes::Default,
            };
            let remotes = repo.push_remotes(&to).map_err(|e| e.to_string())?;
            let mut answer = TagPushes {
                tag: name.clone(),
                pushed: Vec::new(),
                failed: Vec::new(),
            };
            let mut ticker = Ticker::new("tag");
            for remote in &remotes {
                let done = repo
                    .git()
                    .push_tag(remote, &name, &mut |progress| ticker.hear(remote, progress));
                ticker.clear();
                match done {
                    Ok(pushed) => answer.pushed.push(pushed),
                    Err(error) => {
                        say("tag", &format!("not to {remote}"));
                        answer.failed.push(not_pushed(remote, &name, &error));
                    }
                }
            }
            emit(common.json, &answer, tag_pushes_text)?;
            refused(&answer.failed)
        }
    }
}

/// A run on remotes, as the exit status says it: every one took it, or not.
fn refused(failed: &[NotPushed]) -> Result<Answer, String> {
    if failed.is_empty() {
        return Ok(Answer::Yes);
    }
    let names: Vec<&str> = failed.iter().map(|f| f.remote.as_str()).collect();
    Err(format!("not done on {}", names.join(", ")))
}

fn not_pushed(remote: &str, tag: &str, error: &furca_core::Error) -> NotPushed {
    NotPushed {
        remote: remote.to_owned(),
        to: format!("refs/tags/{tag}"),
        error: error.to_string(),
    }
}

/// The branch `name` as it is now.
fn branch_named(repo: &Repository, name: &str) -> Result<Branch, String> {
    repo.refs()
        .map_err(|e| e.to_string())?
        .branches
        .into_iter()
        .find(|branch| branch.name == name)
        .ok_or_else(|| format!("there is no branch {name} here"))
}

/// A local branch on one line: `*` for the one `HEAD` is on, its commit, its
/// name, and what it follows with how far apart the two are.
pub(crate) fn branch_line(branch: &Branch) -> String {
    let mark = if branch.head { '*' } else { ' ' };
    let upstream = match &branch.upstream {
        None => String::new(),
        Some(tracking) => match line::sync(tracking).as_str() {
            "up to date" => format!(" -> {}", tracking.name),
            sync => format!(" -> {} ({sync})", tracking.name),
        },
    };
    format!("{mark} {} {}{upstream}", short(&branch.target), branch.name)
}

/// A tag among the other refs on one line: its commit and its name.
pub(crate) fn tag_line(tag: &Tag) -> String {
    format!("  {} tag: {}", short(&tag.target), tag.name)
}

/// A tag's name, and `annotated` for one with a message of its own.
fn tag_words(tag: &Tag) -> String {
    if tag.annotated {
        format!("{} (annotated)", tag.name)
    } else {
        tag.name.clone()
    }
}

fn branches_text(branches: &[Branch]) -> String {
    if branches.is_empty() {
        return "No branches yet".to_owned();
    }
    branches
        .iter()
        .map(branch_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn tags_text(tags: &[Tag]) -> String {
    if tags.is_empty() {
        return "No tags".to_owned();
    }
    tags.iter()
        .map(|tag| format!("{} {}", short(&tag.target), tag_words(tag)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn switched_text(switched: &Switched) -> String {
    let at = switched.commit.as_deref().map(short).unwrap_or("no commit");
    match (&switched.branch, &switched.upstream) {
        (None, _) => format!("HEAD is on {at}, on no branch."),
        (Some(branch), Some(upstream)) if switched.created => {
            format!("Checked out {branch}, a new branch at {at} following {upstream}.")
        }
        (Some(branch), None) if switched.created => {
            format!("Checked out {branch}, a new branch at {at}.")
        }
        (Some(branch), _) => format!("Checked out {branch} at {at}."),
    }
}

fn deleted_branch_text(deleted: &Deleted) -> String {
    let mut out = format!(
        "Deleted {}, which was at {}",
        deleted.name,
        short(&deleted.target)
    );
    match deleted.lost {
        0 => out.push('.'),
        1 => out.push_str(
            "; the 1 commit only it held went with it, and git's reflog keeps it a while.",
        ),
        lost => out.push_str(&format!(
            "; the {lost} commits only it held went with it, and git's reflog keeps them a while."
        )),
    }
    out
}

fn tag_deleted_text(deleted: &TagDeleted) -> String {
    let mut out = Vec::new();
    if let Some(here) = &deleted.deleted {
        out.push(format!(
            "Deleted {}, which was at {}.",
            here.name,
            short(&here.target)
        ));
    }
    for gone in &deleted.remotes {
        out.push(format!("Taken away from {}.", gone.remote));
    }
    for failed in &deleted.failed {
        out.push(format!(
            "Not taken away from {}: {}",
            failed.remote, failed.error
        ));
    }
    out.join("\n")
}

fn tag_pushes_text(pushes: &TagPushes) -> String {
    let mut out = Vec::new();
    for pushed in &pushes.pushed {
        out.push(format!("Pushed {} to {}", pushed.tag, pushed.remote));
    }
    for failed in &pushes.failed {
        out.push(format!("Not pushed to {}: {}", failed.remote, failed.error));
    }
    out.join("\n")
}
