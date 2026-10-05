//! Reading commits as Conventional Commits: what kind of change each one is,
//! and how far they move the version together.

use serde::Serialize;

use crate::version::Bump;

/// One commit since the last release, read as a Conventional Commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub id: String,
    pub subject: String,
    /// The type, such as `feat` or `fix`; `None` when the message is not a
    /// Conventional Commit at all.
    pub kind: Option<String>,
    pub scope: Option<String>,
    /// A `!` after the type or a `BREAKING CHANGE:` footer.
    pub breaking: bool,
}

impl Change {
    /// Reads `message` (the whole message, not only the subject).
    pub fn read(id: String, subject: String, message: &str) -> Change {
        match git_conventional::Commit::parse(message) {
            Ok(commit) => Change {
                id,
                subject,
                kind: Some(commit.type_().as_str().to_ascii_lowercase()),
                scope: commit.scope().map(|s| s.as_str().to_owned()),
                breaking: commit.breaking(),
            },
            Err(_) => Change {
                id,
                subject,
                kind: None,
                scope: None,
                breaking: false,
            },
        }
    }

    pub fn conventional(&self) -> bool {
        self.kind.is_some()
    }

    fn is(&self, kind: &str) -> bool {
        self.kind.as_deref() == Some(kind)
    }

    /// A dependency update: `chore(deps)`, breaking or not.
    pub fn updates_dependencies(&self) -> bool {
        self.is("chore") && self.scope.as_deref() == Some("deps")
    }

    /// How far this change alone moves the version.
    pub fn bump(&self) -> Bump {
        if self.breaking {
            Bump::Major
        } else if self.is("feat") {
            Bump::Minor
        } else {
            Bump::Patch
        }
    }
}

/// How far a set of changes moves the version: the largest single step, and
/// none at all when there is nothing.
pub fn needed(changes: &[Change]) -> Option<Bump> {
    changes.iter().map(Change::bump).max()
}

/// How many of each kind, for a one-line summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub total: usize,
    pub breaking: usize,
    pub features: usize,
    pub fixes: usize,
    pub other: usize,
    /// Messages that are not Conventional Commits; the global `commit-msg`
    /// hook should have stopped every one of them.
    pub unconventional: usize,
}

impl Tally {
    pub fn of(changes: &[Change]) -> Tally {
        let mut tally = Tally {
            total: changes.len(),
            ..Tally::default()
        };
        for change in changes {
            if !change.conventional() {
                tally.unconventional += 1;
            } else if change.breaking {
                tally.breaking += 1;
            } else if change.is("feat") {
                tally.features += 1;
            } else if change.is("fix") {
                tally.fixes += 1;
            } else {
                tally.other += 1;
            }
        }
        tally
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(message: &str) -> Change {
        let subject = message.lines().next().unwrap_or_default().to_owned();
        Change::read("0".repeat(40), subject, message)
    }

    #[test]
    fn a_footer_breaks_as_surely_as_a_bang() {
        assert!(change("feat!: drop the old flag").breaking);
        assert!(change("chore(deps)!: update gix").breaking);
        assert!(change("fix: keep the order\n\nBREAKING CHANGE: the order is new").breaking);
        assert!(!change("feat: add a flag").breaking);
    }

    #[test]
    fn the_largest_step_wins() {
        let changes = [change("docs: say more"), change("fix: a bug")];
        assert_eq!(needed(&changes), Some(Bump::Patch));
        let changes = [change("fix: a bug"), change("feat(cli): a command")];
        assert_eq!(needed(&changes), Some(Bump::Minor));
        let changes = [change("feat: a"), change("refactor!: b")];
        assert_eq!(needed(&changes), Some(Bump::Major));
        assert_eq!(needed(&[]), None);
    }

    #[test]
    fn a_message_outside_the_convention_still_counts_as_a_change() {
        let merge = change("Merge branch 'topic'");
        assert!(!merge.conventional());
        assert_eq!(merge.bump(), Bump::Patch);
        let tally = Tally::of(&[merge, change("feat: a")]);
        assert_eq!((tally.unconventional, tally.features), (1, 1));
    }

    #[test]
    fn a_dependency_update_is_chore_with_the_deps_scope() {
        assert!(change("chore(deps): update the toolchain").updates_dependencies());
        assert!(change("chore(deps)!: update the toolchain").updates_dependencies());
        assert!(!change("chore: tidy").updates_dependencies());
        assert!(!change("fix(deps): pin a crate").updates_dependencies());
    }
}
