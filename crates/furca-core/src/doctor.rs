//! What is about to be committed that would cost more to mend afterwards.
//!
//! A file that switched its line endings shows up as every line changed, and
//! once it is in the history a `.gitattributes` only stops the next one. A file
//! of some size stays in every clone for good. An assistant's own files are
//! private notes more often than they are the project's. None of this is wrong
//! in itself, so it is said and nothing is refused.
//!
//! Read from the staged list as it stands, with nothing opened of its own: a
//! side not read yet says nothing yet, and what the list reads fills it in.

use serde::Serialize;

use crate::diff::{Content, Endings, FileChange, Side};

/// A file this size or larger is worth a word before it is committed: below
/// what a host refuses, and above what anyone means to put in a history by
/// accident without looking.
pub const LARGE_FILE: u64 = 5 * 1024 * 1024;

/// Names an assistant keeps its notes, memory and settings under, anywhere in a
/// repository.
const ASSISTANT_FILES: [&str; 7] = [
    "CLAUDE.md",
    "CLAUDE.local.md",
    "AGENTS.md",
    "GEMINI.md",
    ".cursorrules",
    ".windsurfrules",
    ".clinerules",
];
const ASSISTANT_FOLDERS: [&str; 7] = [
    ".claude",
    ".cursor",
    ".windsurf",
    ".gemini",
    ".codex",
    ".aider",
    "openspec",
];

/// A word about one file that is staged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Warning {
    /// The file's lines end another way than they did, or a new file's end in
    /// `\r\n`. `before` is `None` for a new file.
    Endings {
        path: String,
        before: Option<Endings>,
        after: Endings,
    },
    /// The file is [`LARGE_FILE`] or more, and not in git LFS.
    Large { path: String, size: u64 },
    /// An assistant's file is going into the repository.
    Assistant { path: String },
}

/// What is worth saying about `staged`, by path.
pub fn doctor(staged: &[FileChange]) -> Vec<Warning> {
    let mut warnings = Vec::new();
    for file in staged {
        let Some(after) = &file.after else { continue };
        if let Some(warning) = endings_drift(file, after) {
            warnings.push(warning);
        }
        let stored = !matches!(after.content, Content::Lfs { .. } | Content::Submodule);
        if let Some(size) = after.size.filter(|size| stored && *size >= LARGE_FILE) {
            warnings.push(Warning::Large {
                path: file.path.clone(),
                size,
            });
        }
        if is_an_assistants(&file.path) {
            warnings.push(Warning::Assistant {
                path: file.path.clone(),
            });
        }
    }
    warnings
}

fn text_endings(side: &Side) -> Option<Endings> {
    match side.content {
        Content::Text { endings } => Some(endings),
        _ => None,
    }
}

fn endings_drift(file: &FileChange, after: &Side) -> Option<Warning> {
    let now = text_endings(after)?;
    let warning = |before| Warning::Endings {
        path: file.path.clone(),
        before,
        after: now,
    };
    match file.before.as_ref() {
        // A new file sets the pattern: `\r\n` here is the drift of tomorrow.
        None => matches!(now, Endings::Crlf | Endings::Mixed).then(|| warning(None)),
        Some(before) => {
            let was = text_endings(before)?;
            // A file of one line has no ending to have changed.
            let changed = was != now && was != Endings::NoEnding && now != Endings::NoEnding;
            changed.then(|| warning(Some(was)))
        }
    }
}

fn is_an_assistants(path: &str) -> bool {
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let last = parts.peek().is_none();
        if last && ASSISTANT_FILES.contains(&part) {
            return true;
        }
        if !last && ASSISTANT_FOLDERS.contains(&part) {
            return true;
        }
    }
    path.ends_with(".github/copilot-instructions.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileMode, FileStatus};

    fn side(content: Content, size: u64) -> Side {
        Side {
            id: Some("0".repeat(40)),
            mode: FileMode::File,
            content,
            size: Some(size),
        }
    }

    fn text(endings: Endings) -> Content {
        Content::Text { endings }
    }

    fn file(path: &str, before: Option<Side>, after: Option<Side>) -> FileChange {
        FileChange {
            path: path.to_owned(),
            status: FileStatus::Modified,
            from: None,
            similarity: None,
            before,
            after,
            lines: None,
        }
    }

    #[test]
    fn lines_that_end_another_way_are_said_and_one_line_files_are_not() {
        let changed = file(
            "a.txt",
            Some(side(text(Endings::Lf), 10)),
            Some(side(text(Endings::Crlf), 12)),
        );
        assert_eq!(
            doctor(&[changed]),
            [Warning::Endings {
                path: "a.txt".to_owned(),
                before: Some(Endings::Lf),
                after: Endings::Crlf,
            }]
        );
        let same = file(
            "b.txt",
            Some(side(text(Endings::Lf), 10)),
            Some(side(text(Endings::Lf), 12)),
        );
        let short = file(
            "c.txt",
            Some(side(text(Endings::NoEnding), 1)),
            Some(side(text(Endings::Crlf), 3)),
        );
        assert_eq!(doctor(&[same, short]), []);
    }

    #[test]
    fn a_new_file_in_crlf_is_said_and_one_in_lf_is_not() {
        let crlf = file("new.txt", None, Some(side(text(Endings::Crlf), 4)));
        let lf = file("fine.txt", None, Some(side(text(Endings::Lf), 4)));
        let warnings = doctor(&[crlf, lf]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(matches!(
            &warnings[0],
            Warning::Endings { before: None, .. }
        ));
    }

    #[test]
    fn a_side_not_read_yet_says_nothing_yet() {
        let unread = file(
            "notes.txt",
            Some(side(Content::Unread, 0)),
            Some(side(Content::Unread, 0)),
        );
        assert_eq!(doctor(&[unread]), []);
    }

    #[test]
    fn a_large_file_is_said_unless_lfs_holds_it() {
        let large = file("big.bin", None, Some(side(Content::Binary, LARGE_FILE)));
        assert_eq!(
            doctor(&[large]),
            [Warning::Large {
                path: "big.bin".to_owned(),
                size: LARGE_FILE,
            }]
        );
        let small = file(
            "small.bin",
            None,
            Some(side(Content::Binary, LARGE_FILE - 1)),
        );
        let lfs = file(
            "movie.mp4",
            None,
            Some(side(
                Content::Lfs {
                    oid: "0".repeat(64),
                },
                LARGE_FILE * 100,
            )),
        );
        assert_eq!(doctor(&[small, lfs]), []);
    }

    #[test]
    fn an_assistants_file_is_said_at_any_depth_and_only_going_in() {
        for path in [
            "CLAUDE.md",
            "sub/AGENTS.md",
            ".claude/settings.json",
            "app/.cursor/rules/a.mdc",
            "openspec/spec.md",
            ".github/copilot-instructions.md",
        ] {
            let warnings = doctor(&[file(path, None, Some(side(text(Endings::Lf), 1)))]);
            assert!(
                warnings.contains(&Warning::Assistant {
                    path: path.to_owned()
                }),
                "{path}: {warnings:?}"
            );
        }
        for path in [
            "docs/claude.md.txt",
            "my.claude",
            "src/agents.rs",
            "CLAUDE.md/x.txt",
        ] {
            let warnings = doctor(&[file(path, None, Some(side(text(Endings::Lf), 1)))]);
            assert_eq!(warnings, [], "{path}");
        }
        let removed = file("CLAUDE.md", Some(side(text(Endings::Lf), 1)), None);
        assert_eq!(doctor(&[removed]), []);
    }
}
