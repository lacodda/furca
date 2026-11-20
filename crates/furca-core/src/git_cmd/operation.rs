//! Merging, cherry-picking and reverting, the steps of an operation halfway,
//! and resolving what one left in conflict: each a run of the system `git`,
//! so hooks, signing and configuration apply as in a terminal (ADR 0023).
//!
//! What is decided first is the core's: what a name to merge means
//! ([`Repository::merge_target`](crate::Repository::merge_target)), that no
//! second operation begins while one is halfway, that nothing goes on while a
//! path is still in conflict. A run that stops at a conflict is not an error:
//! it is a [`Stepped`] that says so, and the repository is halfway.
//!
//! git opens no editor here (`git_command`): a merge, a pick or a revert takes
//! the message git writes for it, and so does each commit a continue makes.

use std::io::ErrorKind;

use gix::index::entry::Stage;

use super::Git;
use crate::conflict::{Resolution, markers};
use crate::diff::{Endings, endings};
use crate::error::Error;
use crate::operation::{FastForward, Proceed, Stepped};
use crate::standing::{Operation, operation};

impl Git<'_> {
    /// Merges `name` - a branch, a remote-tracking branch, a tag, a commit -
    /// into the branch `HEAD` is on, with the message git writes. One that
    /// meets a conflict stops halfway, and says so.
    pub fn merge(&self, name: &str, ff: FastForward) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let target = self.repo.merge_target(name)?;
        let mut args = vec!["merge", "--no-edit"];
        args.extend(match ff {
            FastForward::AsConfigured => None,
            FastForward::Never => Some("--no-ff"),
            FastForward::Only => Some("--ff-only"),
        });
        args.extend(["--", target.as_str()]);
        self.step(&args)
    }

    /// Applies the change each of `revisions` made, in the order given, as a
    /// new commit on `HEAD` with the commit's own message. A merge commit is
    /// taken against its first parent - what it brought into the line it was
    /// merged into. A commit `HEAD` already has is refused: picking it again
    /// changes nothing.
    pub fn cherry_pick(&self, revisions: &[&str]) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let ids = self.commits(revisions)?;
        if let Some(head) = self.repo.inner.head_id().ok().map(|id| id.detach()) {
            for id in &ids {
                if self.repo.count(vec![*id], vec![head])? == 0 {
                    return Err(Error::Operation(format!(
                        "{} is in the history of HEAD already: picking it again changes nothing",
                        short(id)
                    )));
                }
            }
        }
        self.apply("cherry-pick", &ids)
    }

    /// Undoes the change each of `revisions` made, in the order given, each
    /// with a new commit that says so. A merge commit is undone against its
    /// first parent: what it brought in goes.
    pub fn revert(&self, revisions: &[&str]) -> Result<Stepped, Error> {
        self.repo.nothing_halfway()?;
        let ids = self.commits(revisions)?;
        self.apply("revert", &ids)
    }

    /// Continues, skips or aborts the operation halfway here.
    pub fn proceed(&self, how: Proceed) -> Result<Stepped, Error> {
        let Some(halfway) = self.repo.inner.state().map(operation) else {
            return Err(Error::Operation(
                "nothing is halfway here: there is nothing to continue, skip or abort".to_owned(),
            ));
        };
        let command = match halfway {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
            Operation::Am => "am",
            Operation::Bisect => "bisect",
        };
        let args = match (how, halfway) {
            (Proceed::Abort, Operation::Bisect) => ["bisect", "reset"],
            (_, Operation::Bisect) => {
                return Err(Error::Operation(
                    "a bisect goes on as commits are marked good or bad, with git bisect: here it can only be aborted"
                        .to_owned(),
                ));
            }
            (Proceed::Skip, Operation::Merge) => {
                return Err(Error::Operation(
                    "a merge is one step, with nothing to skip: resolve it and continue, or abort it"
                        .to_owned(),
                ));
            }
            (Proceed::Continue, _) => {
                let left = self.repo.conflicts()?.len();
                if left > 0 {
                    return Err(Error::Unresolved { paths: left });
                }
                [command, "--continue"]
            }
            (Proceed::Skip, _) => [command, "--skip"],
            (Proceed::Abort, _) => [command, "--abort"],
        };
        self.step(&args)
    }

    /// Resolves the conflict at `path` as `how` says, and stages it: the
    /// index holds the one version chosen, and the working tree has it, both
    /// written by git through the repository's filters.
    pub fn resolve(&self, path: &str, how: &Resolution) -> Result<(), Error> {
        let stage = |stage| self.repo.staged_at(path, stage);
        let (base, ours, theirs) = (
            stage(Stage::Base)?,
            stage(Stage::Ours)?,
            stage(Stage::Theirs)?,
        );
        // The side whose endings and mode a written resolution takes.
        let Some((like, mode)) = ours
            .clone()
            .or_else(|| theirs.clone())
            .or_else(|| base.clone())
        else {
            return Err(Error::Operation(format!("{path} is not in conflict")));
        };
        match how {
            Resolution::AsIs { force } => {
                let file = self.dir().join(path);
                match std::fs::read(&file) {
                    Ok(bytes) => {
                        if !force && let Some(line) = markers(&bytes) {
                            return Err(Error::Markers {
                                path: path.to_owned(),
                                line,
                            });
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(Error::Write {
                            path: file.display().to_string(),
                            source,
                        });
                    }
                }
                // `--all` stages a file gone from the working tree as gone.
                self.with_paths(&["add", "--all"], &[path])
            }
            Resolution::Ours => match ours {
                Some((id, mode)) => self.put(path, mode, &id),
                None => self.remove(path),
            },
            Resolution::Theirs => match theirs {
                Some((id, mode)) => self.put(path, mode, &id),
                None => self.remove(path),
            },
            Resolution::Delete => self.remove(path),
            Resolution::Text { text } => {
                let crlf = self
                    .blob(&like)
                    .is_some_and(|bytes| endings(&bytes) == Endings::Crlf);
                let bytes = if crlf {
                    with_crlf(text)
                } else {
                    text.as_bytes().to_vec()
                };
                let id = self.run_with(
                    &["hash-object", "-w", "--stdin", "--no-filters"],
                    Some(&bytes),
                )?;
                self.put(path, mode, id.trim())
            }
        }
    }

    /// Puts the blob `id` in the index at `path` as its one version - which
    /// takes the conflict's stages away - and checks it out from there.
    fn put(&self, path: &str, mode: u32, id: &str) -> Result<(), Error> {
        let entry = format!("{mode:o} {id}\t{path}\0");
        self.change_index(
            &["update-index", "-z", "--index-info"],
            Some(entry.as_bytes()),
        )?;
        self.change_index(
            &["checkout-index", "--force", "-z", "--stdin"],
            Some(format!("{path}\0").as_bytes()),
        )
        .map(drop)
    }

    /// Takes `path` out of the index and the working tree: the conflict is
    /// resolved as no file.
    fn remove(&self, path: &str) -> Result<(), Error> {
        self.with_paths(&["rm", "--quiet", "--force"], &[path])
    }

    /// The bytes of the blob `id`, if it can be read.
    fn blob(&self, id: &str) -> Option<Vec<u8>> {
        let id = gix::ObjectId::from_hex(id.as_bytes()).ok()?;
        Some(self.repo.inner.find_blob(id).ok()?.data.clone())
    }

    /// The commits `revisions` name, each resolved now: a branch that moves
    /// while git runs does not change what was asked.
    fn commits(&self, revisions: &[&str]) -> Result<Vec<gix::ObjectId>, Error> {
        if revisions.is_empty() {
            return Err(Error::Operation("name a commit".to_owned()));
        }
        revisions
            .iter()
            .map(|revision| self.repo.resolve(revision))
            .collect()
    }

    /// Picks or reverts `ids` - a merge commit against its first parent.
    fn apply(&self, command: &str, ids: &[gix::ObjectId]) -> Result<Stepped, Error> {
        let hex: Vec<String> = ids.iter().map(ToString::to_string).collect();
        let merges = ids.iter().any(|id| {
            self.repo
                .inner
                .find_commit(*id)
                .is_ok_and(|commit| commit.parent_ids().count() > 1)
        });
        let mut args = vec![command, "--no-edit"];
        if merges {
            args.extend(["--mainline", "1"]);
        }
        // Full ids, which no flag looks like: after a `--` git would take
        // them for paths.
        args.extend(hex.iter().map(String::as_str));
        self.step(&args)
    }

    /// Runs one step of an operation and says where it left the repository.
    /// A run that failed and left an operation halfway that was not before -
    /// or moved `HEAD`, or changed what is in conflict - stopped there, and
    /// that is its answer; one that moved nothing was refused.
    fn step(&self, args: &[&str]) -> Result<Stepped, Error> {
        let before = self.repo.head()?;
        let was = self.repo.inner.state().map(operation);
        let conflicts_before = self.repo.conflicts()?;
        let done = self.change_index(args, None);
        let now = self.repo.inner.state().map(operation);
        let after = self.repo.head()?;
        let conflicts = self.repo.conflicts()?;
        let stopped = match done {
            Ok(_) => None,
            Err(Error::Git { stderr, .. })
                if now.is_some()
                    && (was.is_none()
                        || now != was
                        || after.commit != before.commit
                        || conflicts != conflicts_before) =>
            {
                Some(stderr)
            }
            Err(error) => return Err(error),
        };
        let id = |hex: &Option<String>| {
            hex.as_deref()
                .and_then(|hex| gix::ObjectId::from_hex(hex.as_bytes()).ok())
        };
        let made = match id(&after.commit) {
            Some(to) => self
                .repo
                .count(vec![to], id(&before.commit).into_iter().collect())?,
            None => 0,
        };
        Ok(Stepped {
            operation: now,
            stopped,
            conflicts,
            branch: after.branch,
            from: before.commit,
            to: after.commit,
            made,
        })
    }
}

/// `text` with every line that ends `\n` alone ending `\r\n`.
fn with_crlf(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + text.len() / 32);
    let mut previous = 0u8;
    for &byte in text.as_bytes() {
        if byte == b'\n' && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(byte);
        previous = byte;
    }
    out
}

fn short(id: &gix::ObjectId) -> String {
    id.to_hex_with_len(7).to_string()
}

#[cfg(test)]
mod tests {
    use super::with_crlf;

    #[test]
    fn lines_get_the_endings_the_file_had_once() {
        assert_eq!(with_crlf("a\nb\r\nc"), b"a\r\nb\r\nc");
        assert_eq!(with_crlf(""), b"");
    }
}
