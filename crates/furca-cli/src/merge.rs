//! The commands that bring commits together - `merge`, `cherry-pick`,
//! `revert` - and the ones for what they leave halfway: `continue`, `abort`,
//! `conflicts` and `resolve`. Each is one call into the core, which decides
//! what a name to merge means, refuses a second operation while one is
//! halfway, and has the system `git` do it (ADR 0023).
//!
//! An operation that stops at a conflict answers "no" (exit 1): it did not
//! finish, and the repository is halfway. `conflicts` answers "no" while
//! anything is still in conflict.

use clap::ValueEnum;
use furca_core::{
    BlockKind, Conflict, Conflicted, FastForward, Halfway, LineRange, Operation, Pick, Proceed,
    Resolution, Stepped,
};
use serde::Serialize;

use crate::show::conflict_words;
use crate::{Answer, Common, emit, open, short};

/// What every conflicting block of a file takes.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Take {
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
    /// The common ancestor's lines: neither side's change.
    Base,
}

impl From<Take> for Pick {
    fn from(take: Take) -> Pick {
        match take {
            Take::Ours => Pick::Ours,
            Take::Theirs => Pick::Theirs,
            Take::Both => Pick::Both,
            Take::Base => Pick::Base,
        }
    }
}

/// How `resolve` was asked to resolve a file.
pub(crate) enum Asked {
    AsIs { force: bool },
    Whole { ours: bool },
    Each(Take),
    Delete,
}

/// What an operation was, for the words a step is said in.
#[derive(Clone, Copy)]
enum Doing {
    Merge,
    Pick,
    Revert,
    Proceed(Proceed),
}

pub(crate) fn merge(common: &Common, name: &str, ff: FastForward) -> Result<Answer, String> {
    let stepped = open(&common.repo)?
        .git()
        .merge(name, ff)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, Doing::Merge)
}

pub(crate) fn cherry_pick(common: &Common, revisions: &[String]) -> Result<Answer, String> {
    let revisions: Vec<&str> = revisions.iter().map(String::as_str).collect();
    let stepped = open(&common.repo)?
        .git()
        .cherry_pick(&revisions)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, Doing::Pick)
}

pub(crate) fn revert(common: &Common, revisions: &[String]) -> Result<Answer, String> {
    let revisions: Vec<&str> = revisions.iter().map(String::as_str).collect();
    let stepped = open(&common.repo)?
        .git()
        .revert(&revisions)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, Doing::Revert)
}

pub(crate) fn proceed(common: &Common, how: Proceed) -> Result<Answer, String> {
    let stepped = open(&common.repo)?
        .git()
        .proceed(how)
        .map_err(|e| e.to_string())?;
    said(common, &stepped, Doing::Proceed(how))
}

/// A step as the exit status says it: finished, or stopped halfway.
fn said(common: &Common, stepped: &Stepped, doing: Doing) -> Result<Answer, String> {
    let text = match doing {
        Doing::Merge => merged_text,
        Doing::Pick => picked_text,
        Doing::Revert => reverted_text,
        Doing::Proceed(Proceed::Abort) => aborted_text,
        Doing::Proceed(_) => continued_text,
    };
    emit(common.json, stepped, text)?;
    Ok(if stepped.operation.is_some() {
        Answer::No
    } else {
        Answer::Yes
    })
}

/// What `conflicts` read, for `--json`.
#[derive(Serialize)]
struct Listed {
    /// The operation halfway here, if any.
    halfway: Option<Halfway>,
    /// The paths in conflict, by path.
    conflicts: Vec<Conflicted>,
}

pub(crate) fn conflicts(common: &Common, path: Option<&str>) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    if let Some(path) = path {
        let conflict = repo.conflict(path).map_err(|e| e.to_string())?;
        emit(common.json, &conflict, conflict_text)?;
        return Ok(Answer::No);
    }
    let listed = Listed {
        halfway: repo.halfway().map_err(|e| e.to_string())?,
        conflicts: repo.conflicts().map_err(|e| e.to_string())?,
    };
    emit(common.json, &listed, listed_text)?;
    Ok(if listed.conflicts.is_empty() {
        Answer::Yes
    } else {
        Answer::No
    })
}

/// What `resolve` did, for `--json`.
#[derive(Serialize)]
struct Resolved {
    path: String,
    /// What it was resolved as: `as-is`, `ours` or `theirs` whole, `blocks`
    /// - each conflict from one side - or `delete`.
    took: &'static str,
    /// The side each conflicting block was taken from, for `blocks`.
    each: Option<Pick>,
    /// The paths still in conflict.
    left: Vec<Conflicted>,
    /// Whether an operation is halfway, to go on with once nothing is left.
    halfway: Option<Operation>,
}

pub(crate) fn resolve(common: &Common, path: &str, asked: Asked) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let each = match asked {
        Asked::Each(take) => Some(Pick::from(take)),
        _ => None,
    };
    let (how, took) = match asked {
        Asked::AsIs { force } => (Resolution::AsIs { force }, "as-is"),
        Asked::Whole { ours: true } => (Resolution::Ours, "ours"),
        Asked::Whole { ours: false } => (Resolution::Theirs, "theirs"),
        Asked::Delete => (Resolution::Delete, "delete"),
        Asked::Each(take) => {
            let conflict = repo.conflict(path).map_err(|e| e.to_string())?;
            let text = conflict.merged(take.into()).ok_or_else(|| {
                format!(
                    "{path} is {}, and is taken whole: --ours, --theirs or --delete",
                    conflict_words(conflict.kind)
                )
            })?;
            (Resolution::Text { text }, "blocks")
        }
    };
    repo.git().resolve(path, &how).map_err(|e| e.to_string())?;
    let resolved = Resolved {
        path: path.to_owned(),
        took,
        each,
        left: repo.conflicts().map_err(|e| e.to_string())?,
        halfway: repo
            .halfway()
            .map_err(|e| e.to_string())?
            .map(|h| h.operation),
    };
    emit(common.json, &resolved, resolved_text)
}

/// `a merge` as a sentence begins with it: `A merge`.
fn opening(operation: Operation) -> String {
    let words = operation.words();
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn at(stepped: &Stepped) -> String {
    let on = stepped.branch.as_deref().unwrap_or("HEAD");
    match stepped.to.as_deref() {
        Some(to) => format!("{on} is at {}", short(to)),
        None => format!("{on} has no commit"),
    }
}

fn commits(count: usize) -> String {
    if count == 1 {
        "1 commit".to_owned()
    } else {
        format!("{count} commits")
    }
}

/// An operation that stopped halfway: what is in conflict, or what git
/// said, and what to do next.
fn stopped_text(stepped: &Stepped, operation: Operation) -> String {
    let mut out = Vec::new();
    if stepped.conflicts.is_empty() {
        out.push(format!("{} stopped halfway:", opening(operation)));
        if let Some(said) = &stepped.stopped {
            out.extend(said.lines().map(|line| format!("  {line}")));
        }
        out.push(
            "furca continue --skip leaves this commit out; furca abort puts everything back."
                .to_owned(),
        );
        return out.join("\n");
    }
    let count = stepped.conflicts.len();
    out.push(format!(
        "{} stopped halfway, {} in conflict:",
        opening(operation),
        if count == 1 {
            "1 path".to_owned()
        } else {
            format!("{count} paths")
        }
    ));
    out.extend(conflicted_lines(&stepped.conflicts));
    out.push("Resolve them with furca resolve, then furca continue - or furca abort.".to_owned());
    out.join("\n")
}

fn conflicted_lines(conflicts: &[Conflicted]) -> Vec<String> {
    let width = conflicts
        .iter()
        .map(|c| conflict_words(c.kind).len())
        .max()
        .unwrap_or(0);
    conflicts
        .iter()
        .map(|c| format!("  {:<width$}  {}", conflict_words(c.kind), c.path))
        .collect()
}

fn merged_text(stepped: &Stepped) -> String {
    match stepped.operation {
        Some(operation) => stopped_text(stepped, operation),
        None if stepped.made == 0 => format!("Already up to date: {}.", at(stepped)),
        None => format!("Merged {}: {}.", commits(stepped.made), at(stepped)),
    }
}

fn picked_text(stepped: &Stepped) -> String {
    match stepped.operation {
        Some(operation) => stopped_text(stepped, operation),
        None => format!("Picked {}: {}.", commits(stepped.made), at(stepped)),
    }
}

fn reverted_text(stepped: &Stepped) -> String {
    match stepped.operation {
        Some(operation) => stopped_text(stepped, operation),
        None => format!("Reverted with {}: {}.", commits(stepped.made), at(stepped)),
    }
}

fn continued_text(stepped: &Stepped) -> String {
    match stepped.operation {
        Some(operation) => stopped_text(stepped, operation),
        None => format!("Done: {}.", at(stepped)),
    }
}

fn aborted_text(stepped: &Stepped) -> String {
    format!("Aborted: {}, as it was.", at(stepped))
}

fn listed_text(listed: &Listed) -> String {
    let mut out = Vec::new();
    if let Some(halfway) = &listed.halfway {
        let mut line = format!("{} is halfway", opening(halfway.operation));
        if let Some(branch) = &halfway.branch {
            line.push_str(&format!(" on {branch}"));
        }
        match (&halfway.message, &halfway.commit) {
            (Some(message), _) => line.push_str(&format!(": {message}")),
            (None, Some(commit)) => {
                line.push_str(&format!(": {} {}", short(&commit.id), commit.subject))
            }
            (None, None) => {}
        }
        out.push(format!("{line}."));
    }
    if listed.conflicts.is_empty() {
        out.push(match &listed.halfway {
            Some(halfway) if halfway.can_continue => {
                "Nothing in conflict: furca continue goes on.".to_owned()
            }
            _ => "Nothing in conflict.".to_owned(),
        });
        return out.join("\n");
    }
    out.extend(conflicted_lines(&listed.conflicts));
    out.join("\n")
}

/// One file's conflict: each block that conflicts, the three sides of it as
/// diff3 writes them, with where they are on our side and theirs.
fn conflict_text(conflict: &Conflict) -> String {
    let mut out = vec![format!(
        "{}: {}",
        conflict.path,
        conflict_words(conflict.kind)
    )];
    out.push(match conflict.marked {
        Some(line) => format!("The working tree's file holds git's markers from line {line}."),
        None => format!(
            "The working tree's file holds no markers: furca resolve {} takes it as it is.",
            conflict.path
        ),
    });
    let Some(texts) = &conflict.texts else {
        out.push("Not text: take a side whole with --ours or --theirs, or --delete.".to_owned());
        return out.join("\n");
    };
    if conflict.blocks.is_empty() {
        out.push("One side has no file: keep the other side's whole with --ours or --theirs, or --delete.".to_owned());
        return out.join("\n");
    }
    let lines = |text: &str, range: LineRange| -> Vec<String> {
        text.split_inclusive('\n')
            .skip(range.start as usize)
            .take(range.count as usize)
            .map(|line| line.trim_end_matches(['\n', '\r']).to_owned())
            .collect()
    };
    let total = conflict.conflicts();
    let mut number = 0;
    for block in conflict
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Conflict)
    {
        number += 1;
        out.push(String::new());
        out.push(format!(
            "conflict {number} of {total}: {} of ours, {} of theirs",
            where_lines(block.ours),
            where_lines(block.theirs)
        ));
        out.push("<<<<<<< ours".to_owned());
        out.extend(lines(&texts.ours, block.ours));
        out.push("||||||| base".to_owned());
        out.extend(lines(&texts.base, block.base));
        out.push("=======".to_owned());
        out.extend(lines(&texts.theirs, block.theirs));
        out.push(">>>>>>> theirs".to_owned());
    }
    out.join("\n")
}

/// Where a block's lines are, counted from 1.
fn where_lines(range: LineRange) -> String {
    match range.count {
        0 => format!("nothing before line {}", range.start + 1),
        1 => format!("line {}", range.start + 1),
        count => format!("lines {}-{}", range.start + 1, range.start + count),
    }
}

fn resolved_text(resolved: &Resolved) -> String {
    let first = match resolved.took {
        "as-is" => format!("Resolved {} as the working tree has it.", resolved.path),
        "ours" => format!("Resolved {} with our file whole.", resolved.path),
        "theirs" => format!("Resolved {} with their file whole.", resolved.path),
        "delete" => format!("Resolved {} as deleted.", resolved.path),
        _ => format!(
            "Resolved {}, every conflict with {}.",
            resolved.path,
            match resolved.each {
                Some(Pick::Ours) => "ours",
                Some(Pick::Theirs) => "theirs",
                Some(Pick::Both) => "both sides, ours first",
                Some(Pick::Base) | None => "the ancestor's lines",
            }
        ),
    };
    let next = match (resolved.left.len(), resolved.halfway) {
        (0, Some(_)) => "Nothing left in conflict: furca continue goes on.".to_owned(),
        (0, None) => "Nothing left in conflict.".to_owned(),
        (1, _) => "1 path left in conflict.".to_owned(),
        (left, _) => format!("{left} paths left in conflict."),
    };
    format!("{first}\n{next}")
}

#[cfg(test)]
mod tests {
    use super::where_lines;
    use furca_core::LineRange;

    #[test]
    fn a_block_says_where_its_lines_are_from_one() {
        assert_eq!(where_lines(LineRange { start: 0, count: 1 }), "line 1");
        assert_eq!(where_lines(LineRange { start: 4, count: 3 }), "lines 5-7");
        assert_eq!(
            where_lines(LineRange { start: 2, count: 0 }),
            "nothing before line 3"
        );
    }
}
