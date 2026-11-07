//! A commit message before it is a commit.
//!
//! The release engine reads every commit since the last release as a
//! Conventional Commit, to say how far the version moves and what the changelog
//! holds (`commits.rs`). This reads a message the same way, with the same
//! parser, while it can still be changed: what passes here is what the engine
//! will understand later. The window's composer and `furca message check` both
//! ask this and keep no rules of their own.
//!
//! What a repository's own `commit-msg` hook says is the hook's to say, and
//! [`furca_core::Git::commit_msg_says`] asks it.

use serde::Serialize;

/// The types of the convention, in the order a person picks from.
pub const TYPES: [&str; 11] = [
    "feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert",
];

/// A subject longer than this wraps in a log, and is cut by most hosts.
pub const LONGEST_SUBJECT: usize = 72;

/// Subjects git writes itself, which nobody is asked to reword.
const WRITTEN_BY_GIT: [&str; 5] = ["Merge ", "Revert \"", "fixup! ", "squash! ", "amend! "];

/// A message taken apart, for a form to show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Parts {
    /// The type, such as `feat`; `None` when the subject is not in the
    /// convention, and `description` is then the whole of it.
    pub kind: Option<String>,
    pub scope: Option<String>,
    /// A `!` before the colon. A `BREAKING CHANGE:` footer stays in the body,
    /// where it was written.
    pub breaking: bool,
    pub description: String,
    /// Everything after the subject and the blank line, footers included.
    pub body: String,
}

/// What can be said of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Finding {
    /// Nothing to commit with: git refuses this as well.
    Empty,
    /// The engine would not read it as a Conventional Commit.
    NotConventional,
    /// In the convention's shape, under a type outside [`TYPES`].
    UnknownType { name: String },
    /// The second line is not blank, so git takes it as more of the subject.
    NoBlankLine,
    /// The subject is longer than [`LONGEST_SUBJECT`] characters.
    LongSubject { length: usize, most: usize },
    /// The subject ends in a full stop.
    FinalPeriod,
    /// Letters outside the Latin alphabets: the line writes its history in
    /// English, and a reader of a changelog expects one language.
    NotEnglish,
}

impl Finding {
    /// Whether this is against the convention itself, and not only its style:
    /// what a hook would refuse.
    pub fn breaks_the_convention(&self) -> bool {
        matches!(
            self,
            Self::Empty | Self::NotConventional | Self::UnknownType { .. } | Self::NoBlankLine
        )
    }
}

/// Takes `message` apart. A subject out of the convention is all description.
pub fn parts(message: &str) -> Parts {
    let message = message.trim();
    let (subject, rest) = message.split_once('\n').unwrap_or((message, ""));
    let subject = subject.trim_end();
    let body = rest.trim().to_owned();
    match git_conventional::Commit::parse(subject) {
        Ok(commit) => Parts {
            kind: Some(commit.type_().as_str().to_ascii_lowercase()),
            scope: commit.scope().map(|scope| scope.as_str().to_owned()),
            breaking: commit.breaking(),
            description: commit.description().to_owned(),
            body,
        },
        Err(_) => Parts {
            description: subject.to_owned(),
            body,
            ..Parts::default()
        },
    }
}

/// What can be said of `message`, as a hook or an editor would be handed it:
/// comment lines are not part of it, nor is what follows a scissors line.
pub fn check(message: &str) -> Vec<Finding> {
    let clean = cleaned(message);
    if clean.is_empty() {
        return vec![Finding::Empty];
    }
    let mut lines = clean.lines();
    let subject = lines.next().unwrap_or_default();
    let mut findings = Vec::new();
    if lines.next().is_some_and(|second| !second.trim().is_empty()) {
        findings.push(Finding::NoBlankLine);
    } else {
        match git_conventional::Commit::parse(&clean) {
            Ok(commit) => {
                let name = commit.type_().as_str().to_ascii_lowercase();
                if !TYPES.contains(&name.as_str()) {
                    findings.push(Finding::UnknownType { name });
                }
            }
            Err(_)
                if WRITTEN_BY_GIT
                    .iter()
                    .any(|start| subject.starts_with(start)) => {}
            Err(_) => findings.push(Finding::NotConventional),
        }
    }
    let length = subject.chars().count();
    if length > LONGEST_SUBJECT {
        findings.push(Finding::LongSubject {
            length,
            most: LONGEST_SUBJECT,
        });
    }
    if subject.ends_with('.') {
        findings.push(Finding::FinalPeriod);
    }
    if clean.chars().any(outside_the_latin_alphabets) {
        findings.push(Finding::NotEnglish);
    }
    findings
}

/// A letter past Latin Extended-B: Cyrillic, Greek, CJK and the rest. A name
/// with an accent in a body is English enough.
fn outside_the_latin_alphabets(c: char) -> bool {
    c.is_alphabetic() && u32::from(c) > 0x24F
}

/// The message as git would keep it from an editor.
fn cleaned(message: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in message.lines() {
        if line.starts_with('#') && line.contains(">8") {
            break;
        }
        if !line.starts_with('#') {
            kept.push(line.trim_end());
        }
    }
    kept.join("\n").trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commits::Change;

    fn parts_of(message: &str) -> (Option<String>, Option<String>, bool, String, String) {
        let parts = parts(message);
        (
            parts.kind,
            parts.scope,
            parts.breaking,
            parts.description,
            parts.body,
        )
    }

    #[test]
    fn a_message_comes_apart_into_what_a_form_shows() {
        assert_eq!(
            parts_of("feat(cli)!: drop the flag\n\nWhy it went.\n\nBREAKING CHANGE: it is gone\n"),
            (
                Some("feat".to_owned()),
                Some("cli".to_owned()),
                true,
                "drop the flag".to_owned(),
                "Why it went.\n\nBREAKING CHANGE: it is gone".to_owned()
            )
        );
        assert_eq!(
            parts_of("fix: a bug"),
            (
                Some("fix".to_owned()),
                None,
                false,
                "a bug".to_owned(),
                String::new()
            )
        );
    }

    #[test]
    fn a_subject_out_of_the_convention_is_all_description() {
        assert_eq!(
            parts_of("Tidy up: some of it\n\nAnd more."),
            (
                None,
                None,
                false,
                "Tidy up: some of it".to_owned(),
                "And more.".to_owned()
            )
        );
    }

    #[test]
    fn what_passes_here_is_what_the_engine_reads() {
        for message in [
            "feat: a thing",
            "fix(core)!: a thing\n\nA body.\n\nBREAKING CHANGE: it broke",
            "chore(deps): update gix",
        ] {
            assert_eq!(check(message), [], "{message}");
            let change = Change::read("0".repeat(40), String::new(), message);
            assert!(change.conventional(), "{message}");
        }
        for message in ["a thing", "feat a thing", "feat:", "feat(: x"] {
            let change = Change::read("0".repeat(40), String::new(), message);
            assert!(!change.conventional(), "{message}");
            assert!(
                check(message).contains(&Finding::NotConventional),
                "{message}: {:?}",
                check(message)
            );
        }
    }

    #[test]
    fn what_goes_against_the_convention_is_told_from_what_is_only_style() {
        for finding in [
            Finding::Empty,
            Finding::NotConventional,
            Finding::NoBlankLine,
            Finding::UnknownType {
                name: "wip".to_owned(),
            },
        ] {
            assert!(finding.breaks_the_convention(), "{finding:?}");
        }
        for finding in [
            Finding::LongSubject {
                length: 80,
                most: 72,
            },
            Finding::FinalPeriod,
            Finding::NotEnglish,
        ] {
            assert!(!finding.breaks_the_convention(), "{finding:?}");
        }
    }

    #[test]
    fn nothing_but_comments_is_empty_and_a_scissors_line_ends_the_message() {
        assert_eq!(check(""), [Finding::Empty]);
        assert_eq!(check("  \n\n"), [Finding::Empty]);
        assert_eq!(check("# Please enter a message\n#\n"), [Finding::Empty]);
        assert_eq!(
            check(
                "feat: a thing\n\n# ------------------------ >8 ------------------------\ndiff --git"
            ),
            []
        );
    }

    #[test]
    fn a_body_against_the_subject_is_said() {
        assert_eq!(check("feat: a thing\nmore of it"), [Finding::NoBlankLine]);
    }

    #[test]
    fn an_unknown_type_is_named() {
        assert_eq!(
            check("wip: a thing"),
            [Finding::UnknownType {
                name: "wip".to_owned()
            }]
        );
    }

    #[test]
    fn a_subject_is_counted_in_characters() {
        let at_the_limit = format!("feat: {}", "é".repeat(LONGEST_SUBJECT - 6));
        assert_eq!(check(&at_the_limit), []);
        let over = format!("{at_the_limit}é");
        assert_eq!(
            check(&over),
            [Finding::LongSubject {
                length: LONGEST_SUBJECT + 1,
                most: LONGEST_SUBJECT,
            }]
        );
    }

    #[test]
    fn a_full_stop_and_another_alphabet_are_said_and_an_accent_is_not() {
        assert_eq!(check("fix: a thing."), [Finding::FinalPeriod]);
        assert_eq!(check("fix: исправить"), [Finding::NotEnglish]);
        assert_eq!(check("fix: a thing\n\nСделано."), [Finding::NotEnglish]);
        assert_eq!(check("fix: a thing\n\nThanks to José."), []);
    }

    #[test]
    fn what_git_writes_itself_is_left_alone() {
        for message in [
            "Merge branch 'topic'",
            "Revert \"feat: a thing\"",
            "fixup! feat: a thing",
            "squash! feat: a thing",
            "amend! feat: a thing",
        ] {
            assert_eq!(check(message), [], "{message}");
        }
    }
}
