//! Checking out, and making, renaming and deleting branches and tags: each a
//! run of the system `git`, so its hooks - `post-checkout` among them - and
//! configuration apply as in a terminal (ADR 0022).
//!
//! What is decided first is the core's: what a checkout of a name means
//! ([`Repository::checkout_of`](crate::Repository::checkout_of)), which new
//! branch follows what, and what a deletion would lose
//! ([`Repository::only_on`](crate::Repository::only_on)). git is then told
//! exactly that, with `--track` or `--no-track` and every name after a `--`,
//! so neither its `branch.autoSetupMerge` nor a name that looks like a flag
//! decides it instead.
//!
//! A [`Repository`](crate::Repository) reads the configuration once, when it
//! is opened: after a branch is renamed or set to follow another, open it
//! again to read its upstream as it is.

use super::{Git, Hear};
use crate::branch::{Checkout, Deleted, Switched};
use crate::error::Error;
use crate::remote::TagPushed;

impl Git<'_> {
    /// Checks out what `to` says: a branch, a new branch made at a commit or
    /// a remote-tracking branch, or a commit on no branch. Changes in the
    /// working tree go along where they do not meet the files being checked
    /// out; where they do, git refuses and nothing moves.
    pub fn checkout(&self, to: &Checkout) -> Result<Switched, Error> {
        match to {
            Checkout::Branch { branch } => {
                self.change_index(&["switch", "--no-guess", "--", branch], None)?
            }
            Checkout::New {
                branch,
                from,
                follow,
            } => {
                let start = start_point(from, *follow);
                let track = if *follow { "--track" } else { "--no-track" };
                self.change_index(&["switch", "--create", branch, track, "--", &start], None)?
            }
            Checkout::Detached { commit } => {
                self.change_index(&["switch", "--detach", "--", commit], None)?
            }
        };
        let head = self.repo.head()?;
        Ok(Switched {
            branch: head.branch,
            commit: head.commit,
            created: matches!(to, Checkout::New { .. }),
            // The configuration was read when the handle was opened: a branch
            // this checkout made, and set to follow, is not in it yet.
            upstream: match to {
                Checkout::New { from, follow, .. } => follow.then(|| from.clone()),
                Checkout::Branch { .. } => head.upstream,
                Checkout::Detached { .. } => None,
            },
        })
    }

    /// Makes the branch `name` at `from` - a branch, a tag, a commit - and
    /// leaves `HEAD` where it is. It follows `from` only when that is the
    /// remote-tracking branch of the same name (see
    /// [`Checkout::New`]); otherwise it follows nothing until it is pushed.
    pub fn branch_create(&self, name: &str, from: &str) -> Result<(), Error> {
        let follow = self.repo.follows_from(name, from)?;
        let start = start_point(from, follow);
        let track = if follow { "--track" } else { "--no-track" };
        self.run(&["branch", track, "--", name, &start]).map(drop)
    }

    /// Renames the branch `from` to `to`. What it follows, and its reflog, go
    /// with it, and `HEAD` with them when it is on it. A branch of the new
    /// name already here is refused.
    pub fn branch_rename(&self, from: &str, to: &str) -> Result<(), Error> {
        self.run(&["branch", "--move", "--", from, to]).map(drop)
    }

    /// Deletes the branch `name`. Unless `force`, one that holds commits no
    /// other branch, remote-tracking branch or tag has is refused with
    /// [`Error::Unmerged`], saying how many; with it, they go, and git's
    /// reflog keeps them for a while. The branch `HEAD` is on is refused
    /// either way.
    pub fn branch_delete(&self, name: &str, force: bool) -> Result<Deleted, Error> {
        if self.repo.head()?.branch.as_deref() == Some(name) {
            return Err(Error::Ref(format!(
                "{name} is checked out: check out another branch before deleting it"
            )));
        }
        let target = self
            .repo
            .ref_target(&format!("refs/heads/{name}"))?
            .ok_or_else(|| Error::Ref(format!("there is no branch {name} here")))?;
        let lost = self.repo.only_on(name)?;
        if lost > 0 && !force {
            return Err(Error::Unmerged {
                branch: name.to_owned(),
                commits: lost,
            });
        }
        // git's own rule for `--delete` - merged into the upstream or HEAD -
        // is answered above with the history's own facts, so git is not asked
        // it again.
        self.run(&["branch", "--delete", "--force", "--", name])?;
        Ok(Deleted {
            name: name.to_owned(),
            target,
            lost,
        })
    }

    /// Has the branch `name` follow the branch `upstream` names - a
    /// remote-tracking branch such as `origin/main`, or a local one - or, for
    /// `None`, follow nothing.
    pub fn branch_follow(&self, name: &str, upstream: Option<&str>) -> Result<(), Error> {
        match upstream {
            Some(upstream) => {
                let follow = if self.repo.is_remote_branch(upstream)? {
                    format!("--set-upstream-to=refs/remotes/{upstream}")
                } else {
                    format!("--set-upstream-to={upstream}")
                };
                self.run(&["branch", &follow, "--", name])
            }
            None => self.run(&["branch", "--unset-upstream", "--", name]),
        }
        .map(drop)
    }

    /// Makes the tag `name` on `target`. With a `message` it is an annotated
    /// tag - an object of its own, with the tagger and the date, signed when
    /// `tag.gpgSign` says so - which a push of its branch takes along. Without
    /// one it is a plain tag, a bookmark: a pointer and nothing more, never
    /// signed, which stays here until it is pushed by name.
    pub fn tag(&self, name: &str, target: &str, message: Option<&str>) -> Result<(), Error> {
        match message.map(str::trim).filter(|message| !message.is_empty()) {
            Some(message) => self.run_with(
                &["tag", "--annotate", "--file=-", "--", name, target],
                Some(message.as_bytes()),
            ),
            None => self.run(&["tag", "--no-sign", "--", name, target]),
        }
        .map(drop)
    }

    /// Deletes the tag `name` here. A remote that has it gives it back on the
    /// next fetch, unless it goes from there too: [`Git::push_tag_delete`].
    pub fn tag_delete(&self, name: &str) -> Result<Deleted, Error> {
        let full = format!("refs/tags/{name}");
        let target = self
            .repo
            .ref_target(&full)?
            .ok_or_else(|| Error::Ref(format!("there is no tag {name} here")))?;
        self.run(&["tag", "--delete", "--", name])?;
        Ok(Deleted {
            name: name.to_owned(),
            target,
            lost: 0,
        })
    }

    /// Pushes the tag `name` to `remote`, telling `hear` how far it has got.
    /// A tag of that name there already is not replaced: git refuses.
    pub fn push_tag(&self, remote: &str, name: &str, hear: Hear<'_>) -> Result<TagPushed, Error> {
        let full = format!("refs/tags/{name}");
        if self.repo.ref_target(&full)?.is_none() {
            return Err(Error::Ref(format!("there is no tag {name} here")));
        }
        let refspec = format!("{full}:{full}");
        self.reach(&["push", "--progress", remote, &refspec], Some(hear))?;
        Ok(TagPushed {
            remote: remote.to_owned(),
            tag: name.to_owned(),
            deleted: false,
        })
    }

    /// Takes the tag `name` away from `remote`, telling `hear` how far it has
    /// got; the tag here stays. A remote without it refuses, in git's words.
    pub fn push_tag_delete(
        &self,
        remote: &str,
        name: &str,
        hear: Hear<'_>,
    ) -> Result<TagPushed, Error> {
        let full = format!("refs/tags/{name}");
        self.reach(
            &["push", "--progress", "--delete", remote, &full],
            Some(hear),
        )?;
        Ok(TagPushed {
            remote: remote.to_owned(),
            tag: name.to_owned(),
            deleted: true,
        })
    }
}

/// What git is told to start a branch at: a remote-tracking branch by its
/// full name, so a tag or a local branch of the same short name - which git
/// would take first - does not stand in for it.
fn start_point(from: &str, remote: bool) -> String {
    if remote {
        format!("refs/remotes/{from}")
    } else {
        from.to_owned()
    }
}
