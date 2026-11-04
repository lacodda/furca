//! `furca show`, `furca diff` and `furca status`: a commit as a reader opens
//! it, its lines as a patch, and the working copy.

use furca_core::{
    ConflictKind, Content, Details, FileChange, FileLog, FileStatus, HeadSummary, LineKind, Patch,
    Repository, Side, Status, StatusCache,
};
use serde::Serialize;

/// What `furca status` says: where `HEAD` is, and the working copy against it.
#[derive(Serialize)]
pub struct WorkingCopy {
    #[serde(flatten)]
    head: HeadSummary,
    #[serde(flatten)]
    status: Status,
}

/// Reads the status and then every file in it: a terminal asked for one
/// answer, and has nothing to draw while the rest arrives.
pub fn working_copy(repo: &Repository) -> Result<WorkingCopy, String> {
    let head = repo.head().map_err(|e| e.to_string())?;
    let mut cache = StatusCache::read(repo).map_err(|e| e.to_string())?;
    while cache.read_files(repo, 64) > 0 {}
    Ok(WorkingCopy {
        head,
        status: cache.status().clone(),
    })
}

/// The text `furca status` prints: where `HEAD` is, then a line per file in
/// each list that has any.
pub fn status_text(copy: &WorkingCopy) -> String {
    let head = &copy.head;
    let at = head.commit.as_deref().map(super::short);
    let mut first = match (&head.branch, at) {
        (Some(branch), Some(at)) => format!("On {branch} at {at}"),
        (Some(branch), None) => format!("On {branch}, no commits yet"),
        (None, Some(at)) => format!("HEAD detached at {at}"),
        (None, None) => "HEAD points nowhere".to_owned(),
    };
    if let Some(upstream) = &head.upstream {
        first.push_str(&format!(", tracking {upstream}"));
    }
    let mut out = vec![first];

    let status = &copy.status;
    if !status.conflicted.is_empty() {
        out.push(String::new());
        out.push("Conflicted:".to_owned());
        for conflicted in &status.conflicted {
            out.push(format!(
                "  {}  {}",
                conflicted.path,
                conflict_words(conflicted.kind)
            ));
        }
    }
    for (title, files) in [
        ("Staged:", &status.staged),
        ("Not staged:", &status.unstaged),
        ("Untracked:", &status.untracked),
    ] {
        if files.is_empty() {
            continue;
        }
        out.push(String::new());
        out.push(title.to_owned());
        let width = files
            .iter()
            .map(|file| shown_path(file).chars().count())
            .max()
            .unwrap_or(0);
        for file in files {
            out.push(format!(
                "  {} {:<width$}  {}",
                letter(file.status),
                shown_path(file),
                what_moved(file)
            ));
        }
    }
    out.join("\n")
}

fn conflict_words(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::BothModified => "both modified",
        ConflictKind::BothAdded => "both added",
        ConflictKind::BothDeleted => "both deleted",
        ConflictKind::AddedByUs => "added by us",
        ConflictKind::AddedByThem => "added by them",
        ConflictKind::DeletedByUs => "deleted by us",
        ConflictKind::DeletedByThem => "deleted by them",
    }
}

/// The text `furca show` prints: the commit, its message, and a line per
/// file with what it is and how much moved.
pub fn details_text(details: &Details) -> String {
    let commit = &details.commit;
    let mut out = vec![format!("commit {}", commit.id)];
    if commit.parents.len() > 1 {
        let parents: Vec<&str> = commit.parents.iter().map(|id| super::short(id)).collect();
        out.push(format!("Merge:  {}", parents.join(" ")));
    }
    out.push(format!(
        "Author: {} <{}>  {}",
        commit.author.name, commit.author.email, commit.author.time
    ));
    if commit.committer.name != commit.author.name || commit.committer.email != commit.author.email
    {
        out.push(format!(
            "Commit: {} <{}>  {}",
            commit.committer.name, commit.committer.email, commit.committer.time
        ));
    }
    out.push(String::new());
    out.extend(details.message.lines().map(|line| format!("    {line}")));
    out.push(String::new());

    let files = &details.changes.files;
    let width = files
        .iter()
        .map(|file| shown_path(file).chars().count())
        .max()
        .unwrap_or(0);
    for file in files {
        out.push(format!(
            " {} {:<width$}  {}",
            letter(file.status),
            shown_path(file),
            what_moved(file)
        ));
    }
    let added: u32 = files.iter().filter_map(|f| f.lines).map(|l| l.added).sum();
    let removed: u32 = files
        .iter()
        .filter_map(|f| f.lines)
        .map(|l| l.removed)
        .sum();
    out.push(format!(
        "{} {}, +{added} -{removed}",
        files.len(),
        if files.len() == 1 { "file" } else { "files" }
    ));
    if let Some(parent) = &details.changes.parent
        && commit.parents.len() > 1
    {
        out.push(format!(
            "compared with the first parent, {}",
            super::short(parent)
        ));
    }
    out.join("\n")
}

/// The text `furca diff` prints: each file's patch, as `git diff` writes one.
pub fn patches_text(patches: &[Patch]) -> String {
    if patches.is_empty() {
        return "No files changed".to_owned();
    }
    let mut out = Vec::new();
    for patch in patches {
        let file = &patch.file;
        let before = file.from.as_deref().unwrap_or(&file.path);
        out.push(format!("diff {}", shown_path(file)));
        if let Some(similarity) = file.similarity {
            out.push(format!("similarity {similarity}%"));
        }
        if file.lines.is_none() {
            out.push(format!("{}: {}", file.path, what_moved(file)));
            continue;
        }
        out.push(match file.before {
            Some(_) => format!("--- a/{before}"),
            None => "--- /dev/null".to_owned(),
        });
        out.push(match file.after {
            Some(_) => format!("+++ b/{}", file.path),
            None => "+++ /dev/null".to_owned(),
        });
        for hunk in &patch.hunks {
            out.push(format!(
                "@@ -{} +{} @@",
                header(hunk.before.start, hunk.before.count),
                header(hunk.after.start, hunk.after.count)
            ));
            for line in &hunk.lines {
                let mark = match line.kind {
                    LineKind::Context => ' ',
                    LineKind::Removed => '-',
                    LineKind::Added => '+',
                };
                out.push(format!("{mark}{}", line.text));
            }
        }
    }
    out.join("\n")
}

/// The text `furca log -- PATH` prints: a line per commit that changed the
/// file, with what it did to it.
pub fn file_log_text(log: &FileLog) -> String {
    if log.commits.is_empty() {
        return "No commit changed this path".to_owned();
    }
    let mut lines: Vec<String> = log
        .commits
        .iter()
        .map(|revision| {
            let commit = &revision.commit;
            let date = commit.author.time.get(..10).unwrap_or(&commit.author.time);
            let path = match &revision.from {
                Some(from) => format!("{from} -> {}", revision.path),
                None => revision.path.clone(),
            };
            format!(
                "{} {date} {} {}  {}  ({path})",
                super::short(&commit.id),
                letter(revision.status),
                commit.author.name,
                commit.subject
            )
        })
        .collect();
    if log.truncated {
        lines.push(format!(
            "... more history; raise --limit (now {})",
            log.commits.len()
        ));
    }
    lines.join("\n")
}

/// A hunk's side as the `@@` line writes it: 1-based, and a run of no lines
/// names the line before it.
fn header(start: u32, count: u32) -> String {
    let first = if count == 0 { start } else { start + 1 };
    if count == 1 {
        first.to_string()
    } else {
        format!("{first},{count}")
    }
}

fn letter(status: FileStatus) -> char {
    match status {
        FileStatus::Added => 'A',
        FileStatus::Deleted => 'D',
        FileStatus::Modified => 'M',
        FileStatus::Renamed => 'R',
        FileStatus::Copied => 'C',
    }
}

fn shown_path(file: &FileChange) -> String {
    match &file.from {
        Some(from) => format!("{from} -> {}", file.path),
        None => file.path.clone(),
    }
}

/// How much a file moved: its lines, or what it is and its size when it has
/// none.
fn what_moved(file: &FileChange) -> String {
    if let Some(lines) = file.lines {
        return format!("+{} -{}", lines.added, lines.removed);
    }
    let side = |side: &Option<Side>| match side {
        None => "nothing".to_owned(),
        Some(side) => match &side.content {
            Content::Submodule => match &side.id {
                Some(id) => format!("submodule at {}", super::short(id)),
                None => "a repository".to_owned(),
            },
            Content::Lfs { .. } => format!("git LFS, {}", size(side.size.unwrap_or(0))),
            Content::Binary => format!("binary, {}", size(side.size.unwrap_or(0))),
            Content::Text { .. } => format!("text, {}", size(side.size.unwrap_or(0))),
            // Could not be read; `furca diff` on it says why.
            Content::Unread => "could not be read".to_owned(),
        },
    };
    match (&file.before, &file.after) {
        (None, Some(_)) => side(&file.after),
        (Some(_), None) => side(&file.before),
        _ => format!("{} -> {}", side(&file.before), side(&file.after)),
    }
}

/// Bytes as a person reads them, in powers of 1024.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
