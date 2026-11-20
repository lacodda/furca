//! Merging, picking and reverting commits, and the operations git leaves
//! halfway until they are continued, skipped or aborted.
//!
//! An operation that stops at a conflict has not failed: it is halfway, and
//! that is a state of the repository like any other - what is being brought
//! in, what is left to resolve - which every door reads the same way
//! ([`Repository::halfway`]). A step of one says where it left the repository
//! ([`Stepped`]); only a step git refused, which moved nothing, is an error
//! (ADR 0023).

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::error::Error;
use crate::log::Commit;
use crate::standing::{Operation, operation};
use crate::status::Conflicted;

/// Whether a merge may move the branch along to what it merges, when that is
/// all it would take: git's own choice, with its flags' names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum FastForward {
    /// As `merge.ff` says: a fast-forward where one will do, unless the
    /// configuration asks for a merge commit every time.
    #[default]
    AsConfigured,
    /// A merge commit every time: `--no-ff`.
    Never,
    /// Only a fast-forward, and a refusal when the two have parted:
    /// `--ff-only`.
    Only,
}

/// What to do with an operation halfway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Proceed {
    /// Go on, once every conflict is resolved: the commit it stopped at is
    /// made with the message git wrote for it, and the next one is taken up.
    Continue,
    /// Leave out the commit it stopped at, and go on with the next.
    Skip,
    /// Put everything back as it was before the operation began.
    Abort,
}

/// An operation halfway, as it stands now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Halfway {
    pub operation: Operation,
    /// The branch it lands on: the one `HEAD` is on, or the one a rebase is
    /// rewriting. `None` on no branch.
    pub branch: Option<String>,
    /// The commit it is bringing in: the one being merged, picked, reverted
    /// or replayed. `None` where git names none - a bisect, `git am`, a
    /// sequence of picks between two of its commits.
    pub commit: Option<Commit>,
    /// The first line of the message git wrote for the commit that finishes
    /// it, such as `Merge branch 'feature'`; `None` when it wrote none.
    pub message: Option<String>,
    /// Paths still to resolve.
    pub conflicts: usize,
    /// Whether it can be continued: anything but a bisect.
    pub can_continue: bool,
    /// Whether the commit it stopped at can be left out: a pick, a revert, a
    /// rebase or `git am` - not a merge, which is one step.
    pub can_skip: bool,
}

/// Where a step of an operation left the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Stepped {
    /// The operation still halfway; `None` once it finished, or was aborted.
    pub operation: Option<Operation>,
    /// What git said when it stopped halfway: a conflict, or a commit that
    /// came out empty. `None` when it did not stop.
    pub stopped: Option<String>,
    /// The paths left to resolve, by path.
    pub conflicts: Vec<Conflicted>,
    /// The branch `HEAD` is on; `None` on no branch.
    pub branch: Option<String>,
    /// The commit `HEAD` was on before; `None` before the first commit.
    pub from: Option<String>,
    /// The commit it is on now; the same as `from` when nothing moved.
    pub to: Option<String>,
    /// Commits `HEAD` has now that it did not have before: what a pick or a
    /// revert made, and what a merge brought in, its merge commit with them.
    pub made: usize,
}

impl Operation {
    /// The operation as a sentence names it: `a merge`, `a cherry-pick`.
    pub fn words(self) -> &'static str {
        match self {
            Operation::Merge => "a merge",
            Operation::Rebase => "a rebase",
            Operation::CherryPick => "a cherry-pick",
            Operation::Revert => "a revert",
            Operation::Bisect => "a bisect",
            Operation::Am => "a git am",
        }
    }
}

impl Repository {
    /// The operation git has halfway here, and what it is bringing in;
    /// `None` when nothing is halfway.
    pub fn halfway(&self) -> Result<Option<Halfway>, Error> {
        let Some(operation) = self.inner.state().map(operation) else {
            return Ok(None);
        };
        let dir = self.git_dir();
        let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
        let first_line = |text: String| {
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .map(ToOwned::to_owned)
        };
        let marker = match operation {
            Operation::Merge => Some("MERGE_HEAD"),
            Operation::CherryPick => Some("CHERRY_PICK_HEAD"),
            Operation::Revert => Some("REVERT_HEAD"),
            Operation::Rebase => Some("REBASE_HEAD"),
            Operation::Bisect | Operation::Am => None,
        };
        let commit = marker
            .and_then(read)
            .and_then(first_line)
            .and_then(|id| self.commit(&id).ok());
        let branch = match operation {
            Operation::Rebase => ["rebase-merge/head-name", "rebase-apply/head-name"]
                .into_iter()
                .find_map(read)
                .and_then(first_line)
                .map(|name| name.trim_start_matches("refs/heads/").to_owned())
                .filter(|name| name != "detached HEAD"),
            _ => self.head()?.branch,
        };
        Ok(Some(Halfway {
            operation,
            branch,
            commit,
            message: read("MERGE_MSG").and_then(first_line),
            conflicts: self.conflicts()?.len(),
            can_continue: operation != Operation::Bisect,
            can_skip: matches!(
                operation,
                Operation::CherryPick | Operation::Revert | Operation::Rebase | Operation::Am
            ),
        }))
    }

    /// What git is told to merge for `name`: a ref by its full name, such as
    /// `refs/tags/v1.0.0`; or a local branch, a remote-tracking branch or a
    /// tag of that name - in that order, as a checkout looks for one; or else
    /// the commit `name` resolves to. A ref is given to git by its short name
    /// when git would find that same ref by it, so the merge's message says
    /// `Merge branch 'feature'`; by its full name when another ref would
    /// stand in for it.
    pub fn merge_target(&self, name: &str) -> Result<String, Error> {
        let full = if name.starts_with("refs/") && self.has_ref(name)? {
            Some(name.to_owned())
        } else {
            let mut found = None;
            for kind in ["heads", "remotes", "tags"] {
                let full = format!("refs/{kind}/{name}");
                if self.has_ref(&full)? {
                    found = Some(full);
                    break;
                }
            }
            found
        };
        let Some(full) = full else {
            return Ok(self.resolve(name)?.to_string());
        };
        let short = ["refs/heads/", "refs/remotes/", "refs/tags/"]
            .into_iter()
            .find_map(|prefix| full.strip_prefix(prefix))
            .unwrap_or(&full);
        if full.starts_with("refs/heads/") && self.head()?.branch.as_deref() == Some(short) {
            return Err(Error::Operation(format!(
                "{short} is the branch HEAD is on: there is nothing to merge into it from itself"
            )));
        }
        Ok(
            if self.first_found(short)?.as_deref() == Some(full.as_str()) {
                short.to_owned()
            } else {
                full
            },
        )
    }

    /// The ref git takes `name` for, looking where `git merge` looks.
    fn first_found(&self, name: &str) -> Result<Option<String>, Error> {
        for full in [
            format!("refs/{name}"),
            format!("refs/tags/{name}"),
            format!("refs/heads/{name}"),
            format!("refs/remotes/{name}"),
            format!("refs/remotes/{name}/HEAD"),
        ] {
            if self.has_ref(&full)? {
                return Ok(Some(full));
            }
        }
        Ok(None)
    }

    /// Refuses to begin an operation while another is halfway: git would
    /// refuse too, in words that do not say what to do.
    pub(crate) fn nothing_halfway(&self) -> Result<(), Error> {
        match self.inner.state().map(operation) {
            Some(operation) => Err(Error::Halfway { operation }),
            None => Ok(()),
        }
    }
}
