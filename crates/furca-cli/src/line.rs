//! `furca line`: where every repository of the line stands, side by side -
//! what is uncommitted, unpushed and unreleased in each, read from this
//! machine alone.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use furca_core::{Repository, Standing};
use serde::Serialize;

/// Where the list of repositories came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The projects the owner's record keeps.
    Record,
    /// The paths and patterns on the command line.
    Paths,
}

/// The line, read.
#[derive(Debug, Serialize)]
pub struct Line {
    pub source: Source,
    /// Every repository was read and is settled.
    pub settled: bool,
    pub repositories: Vec<Entry>,
}

/// One repository of the line: where it stands, or why it could not be read.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    /// Read, and nothing in it exists on this machine alone.
    pub settled: bool,
    #[serde(flatten)]
    pub reading: Reading,
}

/// Serialized as one key beside the name: `standing` or `error`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reading {
    Standing(Box<Standing>),
    Error(String),
}

/// A repository to read: a name to show it by, and its root.
pub struct Target {
    pub name: String,
    pub path: PathBuf,
}

/// The repositories `patterns` name, each once, in the order named. A
/// pattern with `*` or `?` stands for the repositories at the directories
/// it matches; a plain path names the repository it is in, as `-C` does.
/// A pattern that matches no repository, or a path in none, is an error:
/// it is a mistake on the command line, not a fact about the line.
pub fn targets(patterns: &[String]) -> Result<Vec<Target>, String> {
    let mut seen = HashSet::new();
    let mut targets = Vec::new();
    for pattern in patterns {
        let roots = if pattern.contains(['*', '?']) {
            let roots = furca_core::repositories(pattern);
            if roots.is_empty() {
                return Err(format!("`{pattern}` matches no repository"));
            }
            roots
        } else {
            let repo = Repository::open(pattern).map_err(|e| format!("{pattern}: {e}"))?;
            let root = repo.workdir().unwrap_or_else(|| repo.git_dir());
            vec![std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf())]
        };
        for root in roots {
            let same = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            if seen.insert(same) {
                targets.push(Target {
                    name: name_of(&root),
                    path: root,
                });
            }
        }
    }
    Ok(targets)
}

/// The repositories the owner's record keeps.
pub fn recorded() -> Result<Vec<Target>, String> {
    let kept = furca_release::Rigger.repositories().map_err(|why| {
        format!("{why}; name the repositories instead, such as `furca line \"DIR/*\"`")
    })?;
    Ok(kept
        .into_iter()
        .map(|kept| Target {
            name: kept.name,
            path: kept.path,
        })
        .collect())
}

/// Reads every target, several at a time: a line of twenty repositories
/// costs about as much as its slowest one.
pub fn read(source: Source, targets: Vec<Target>) -> Line {
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .min(targets.len());
    let next = AtomicUsize::new(0);
    let (send, receive) = mpsc::channel();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (send, next, targets) = (send.clone(), &next, &targets);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(target) = targets.get(index) else {
                        break;
                    };
                    if send.send((index, entry(target))).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(send);
    let mut read: Vec<(usize, Entry)> = receive.into_iter().collect();
    read.sort_by_key(|(index, _)| *index);
    let repositories: Vec<Entry> = read.into_iter().map(|(_, entry)| entry).collect();
    Line {
        source,
        settled: repositories.iter().all(|entry| entry.settled),
        repositories,
    }
}

/// Reads one repository, opened exactly at its root: a recorded path that
/// is no longer a repository must not read as one further up.
fn entry(target: &Target) -> Entry {
    let reading = Repository::at(&target.path).and_then(|repo| repo.standing());
    let (settled, reading) = match reading {
        Ok(standing) => (standing.settled(), Reading::Standing(Box::new(standing))),
        Err(error) => (false, Reading::Error(error.to_string())),
    };
    Entry {
        name: target.name.clone(),
        path: target.path.clone(),
        settled,
        reading,
    }
}

/// The name a repository found by its path goes by: its directory's.
fn name_of(root: &Path) -> String {
    root.file_name().map_or_else(
        || root.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The line as a table, one repository a row, and a sentence under it.
pub fn text(line: &Line) -> String {
    if line.repositories.is_empty() {
        return "No repositories to read.".to_owned();
    }
    let header = [
        "REPOSITORY",
        "BRANCH",
        "UPSTREAM",
        "TAG",
        "TAGGED",
        "SINCE",
        "ONLY HERE",
    ];
    let rows: Vec<Row> = line
        .repositories
        .iter()
        .map(|entry| match &entry.reading {
            Reading::Standing(standing) => Row::Read(row(&entry.name, standing)),
            Reading::Error(error) => Row::Unread(&entry.name, error),
        })
        .collect();

    let mut widths = header.map(str::len);
    for row in &rows {
        match row {
            Row::Read(cells) => {
                for (width, cell) in widths.iter_mut().zip(cells) {
                    *width = (*width).max(cell.chars().count());
                }
            }
            Row::Unread(name, _) => widths[0] = widths[0].max(name.chars().count()),
        }
    }

    let mut out = vec![cells_line(&header.map(str::to_owned), &widths)];
    for row in &rows {
        out.push(match row {
            Row::Read(cells) => cells_line(cells, &widths),
            Row::Unread(name, error) => format!("{name:<width$}  {error}", width = widths[0]),
        });
    }
    out.push(String::new());
    out.push(summary(line));
    out.join("\n")
}

/// A row of the table: the cells of a repository read, or its name and why
/// it could not be read, written across the row.
enum Row<'a> {
    Read([String; 7]),
    Unread(&'a str, &'a str),
}

/// The cells of a row, left-aligned except the count of commits since the
/// tag, with the last cell left unpadded.
fn cells_line(cells: &[String; 7], widths: &[usize; 7]) -> String {
    let mut line = String::new();
    for (index, (cell, width)) in cells.iter().zip(widths).enumerate() {
        if index > 0 {
            line.push_str("  ");
        }
        let pad = width.saturating_sub(cell.chars().count());
        match index {
            5 => {
                line.push_str(&" ".repeat(pad));
                line.push_str(cell);
            }
            6 => line.push_str(cell),
            _ => {
                line.push_str(cell);
                line.push_str(&" ".repeat(pad));
            }
        }
    }
    line.trim_end().to_owned()
}

fn row(name: &str, standing: &Standing) -> [String; 7] {
    let branch = standing
        .branch
        .clone()
        .unwrap_or_else(|| "(detached)".to_owned());
    let upstream = match &standing.upstream {
        None => "-".to_owned(),
        Some(tracking) => match (tracking.ahead, tracking.behind) {
            (Some(ahead), Some(behind)) => sync(ahead, behind),
            _ => "gone".to_owned(),
        },
    };
    let (tag, tagged, since) = match &standing.tag {
        None => ("-".to_owned(), "-".to_owned(), "-".to_owned()),
        Some(tag) => (
            tag.name.clone(),
            tag.time.get(..10).unwrap_or(&tag.time).to_owned(),
            tag.since.to_string(),
        ),
    };
    [
        name.to_owned(),
        branch,
        upstream,
        tag,
        tagged,
        since,
        only_here(standing),
    ]
}

fn sync(ahead: usize, behind: usize) -> String {
    match (ahead, behind) {
        (0, 0) => "up to date".to_owned(),
        (ahead, 0) => format!("{ahead} ahead"),
        (0, behind) => format!("{behind} behind"),
        (ahead, behind) => format!("{ahead} ahead, {behind} behind"),
    }
}

/// What the repository holds that is nowhere else, or `-`.
fn only_here(standing: &Standing) -> String {
    let mut parts = Vec::new();
    for (count, what) in [
        (standing.changed, "changed"),
        (standing.untracked, "untracked"),
        (standing.unpushed, "unpushed"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {what}"));
        }
    }
    if standing.stashes > 0 {
        parts.push(plural(standing.stashes, "stash", "stashes"));
    }
    if let Some(operation) = standing.operation {
        parts.push(format!("{} in progress", crate::serde_word(&operation)));
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(", ")
    }
}

fn summary(line: &Line) -> String {
    let total = line.repositories.len();
    let unread = line
        .repositories
        .iter()
        .filter(|entry| matches!(entry.reading, Reading::Error(_)))
        .count();
    let settled = line.repositories.iter().filter(|e| e.settled).count();
    let holding = total - settled - unread;
    let all = plural(total, "repository", "repositories");
    if settled == total {
        return format!("{all}, all settled.");
    }
    let mut parts = vec![format!("{settled} settled")];
    if holding > 0 {
        parts.push(format!("{holding} with work only here"));
    }
    if unread > 0 {
        parts.push(format!("{unread} could not be read"));
    }
    format!("{all}: {}.", parts.join(", "))
}

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_path_inside_a_repository_is_not_read_as_that_repository() {
        let dir = tempfile::tempdir().expect("temp dir");
        let status = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .expect("git runs");
        assert!(status.success());
        let inside = dir.path().join("moved-away");
        std::fs::create_dir_all(&inside).expect("dir");

        let entry = entry(&Target {
            name: "app".to_owned(),
            path: inside,
        });
        assert!(!entry.settled);
        let json = serde_json::to_value(&entry).expect("JSON");
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("could not open the repository")),
            "{json}"
        );
        assert!(json.get("standing").is_none(), "{json}");
    }

    #[test]
    fn the_summary_counts_what_is_settled_held_and_unread() {
        let unread = |name: &str| Entry {
            name: name.to_owned(),
            path: PathBuf::from(name),
            settled: false,
            reading: Reading::Error("could not open the repository: gone".to_owned()),
        };
        let line = Line {
            source: Source::Paths,
            settled: false,
            repositories: vec![unread("a")],
        };
        assert_eq!(
            summary(&line),
            "1 repository: 0 settled, 1 could not be read."
        );
        let text = text(&line);
        assert!(
            text.lines()
                .nth(1)
                .is_some_and(|row| row.starts_with("a           could not open")),
            "{text}"
        );
    }
}
