//! Branches and tags as things to make, move and take away: what checking a
//! name out means, which new branch follows what, and what deleting one would
//! lose - the core's one answer for every door, as
//! [`Repository::destinations`] is for a push (ADR 0022).

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::error::{Error, wrap};

/// What a checkout makes of `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "to", rename_all = "kebab-case")]
pub enum Checkout {
    /// `HEAD` on the local branch `branch`.
    Branch { branch: String },
    /// A new local branch `branch`, made at `from` and checked out. It
    /// follows `from` when that is a remote-tracking branch and `branch` is
    /// its name on its remote - a remote branch checked out here - and
    /// follows nothing otherwise, until it is pushed.
    New {
        branch: String,
        from: String,
        follow: bool,
    },
    /// `HEAD` on the commit `commit` itself, on no branch.
    Detached { commit: String },
}

/// Where `HEAD` is after a checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Switched {
    /// The branch `HEAD` is on; `None` when it is detached.
    pub branch: Option<String>,
    /// The commit `HEAD` is on.
    pub commit: Option<String>,
    /// The branch was made by this checkout.
    pub created: bool,
    /// The remote-tracking branch the branch follows, such as `origin/main`.
    pub upstream: Option<String>,
}

/// A branch or a tag that was deleted, and where it was: what it takes to
/// make it again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Deleted {
    /// Its short name, such as `topic` or `v1.0.0`.
    pub name: String,
    /// The commit it pointed at.
    pub target: String,
    /// The commits only it held, which nothing names now. git keeps them in
    /// its reflog for a while; `0` for a tag, or a branch merged elsewhere.
    pub lost: usize,
}

impl Repository {
    /// What checking out `name` means. The local branch of that name; or,
    /// for a remote-tracking branch such as `origin/feature`, the local
    /// branch `feature` - made to follow it when there is none yet, and
    /// checked out as it is when it follows it already. A local branch of
    /// that name that follows something else is refused: checking it out
    /// would not be checking out the remote's. Anything that is not a branch
    /// is refused too: a commit or a tag is checked out with
    /// [`Repository::checkout_detached`], on no branch, and only when asked.
    pub fn checkout_of(&self, name: &str) -> Result<Checkout, Error> {
        if self.has_ref(&format!("refs/heads/{name}"))? {
            return Ok(Checkout::Branch {
                branch: name.to_owned(),
            });
        }
        if !self.has_ref(&format!("refs/remotes/{name}"))? {
            return Err(Error::Ref(format!(
                "{name} is no branch here: a commit or a tag is checked out on no branch, \
                 detached, and only when that is asked for"
            )));
        }
        let local = self.name_on_its_remote(name);
        if !self.has_ref(&format!("refs/heads/{local}"))? {
            return Ok(Checkout::New {
                branch: local,
                from: name.to_owned(),
                follow: true,
            });
        }
        match self.upstream_of(full_branch(&local)?.as_ref()) {
            Some(follows) if follows == name => Ok(Checkout::Branch { branch: local }),
            follows => Err(Error::Ref(format!(
                "there is a branch {local} here already, {}: check it out by its own name, \
                 or make a branch of another name from {name}",
                follows.map_or_else(
                    || "following nothing".to_owned(),
                    |up| format!("following {up}")
                )
            ))),
        }
    }

    /// A new branch `name` made at `from` - a branch, a tag, a commit - and
    /// checked out. It follows `from` only when that is the remote-tracking
    /// branch of the same name; see [`Checkout::New`].
    pub fn checkout_new(&self, name: &str, from: &str) -> Result<Checkout, Error> {
        self.resolve(from)?;
        Ok(Checkout::New {
            branch: name.to_owned(),
            from: from.to_owned(),
            follow: self.follows_from(name, from)?,
        })
    }

    /// `HEAD` on the commit `revision` names - a commit, a tag, a branch's
    /// tip - and on no branch.
    pub fn checkout_detached(&self, revision: &str) -> Result<Checkout, Error> {
        Ok(Checkout::Detached {
            commit: self.resolve(revision)?.to_string(),
        })
    }

    /// How many commits only the branch `name` holds: what deleting it would
    /// leave that no other branch, remote-tracking branch or tag reaches, nor
    /// `HEAD` when it is elsewhere. A branch merged anywhere, or pushed, holds
    /// none.
    ///
    /// This is a fact of the history rather than git's own rule for `git
    /// branch --delete`, which asks only whether the branch is merged into
    /// its upstream or into `HEAD`: a branch a tag keeps is lost to nobody,
    /// and one merged into its upstream but nowhere else is lost to nobody
    /// either.
    pub fn only_on(&self, name: &str) -> Result<usize, Error> {
        let full = full_branch(name)?;
        let Some(tip) = self.peeled_commit(&format!("refs/heads/{name}"))? else {
            return Err(Error::Ref(format!("there is no branch {name} here")));
        };
        let platform = self.inner.references().map_err(wrap(Error::References))?;
        let packed = self.packed_refs()?;
        let packed = packed.as_ref().map(|buffer| &***buffer);
        let mut others = Vec::new();
        let kinds = [
            platform.local_branches(),
            platform.remote_branches(),
            platform.tags(),
        ];
        for iter in kinds {
            for reference in iter.map_err(wrap(Error::References))? {
                let mut reference = reference.map_err(wrap(Error::References))?;
                if reference.name() == full.as_ref() {
                    continue;
                }
                if let Ok(id) = reference.peel_to_id_packed(packed)
                    && self.is_commit(id.detach())
                {
                    others.push(id.detach());
                }
            }
        }
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        if head.referent_name() != Some(full.as_ref())
            && let Some(id) = head.id()
        {
            others.push(id.detach());
        }
        self.count(vec![tip], others)
    }

    /// Whether a new branch `name` made at `from` follows it: only when
    /// `from` is a remote-tracking branch, and `name` is the name it has on
    /// its remote. A branch made from `origin/main` under another name would
    /// otherwise follow `main`, and its first push would go there.
    pub(crate) fn follows_from(&self, name: &str, from: &str) -> Result<bool, Error> {
        Ok(self.is_remote_branch(from)? && self.name_on_its_remote(from) == name)
    }

    /// Whether `name` is a remote-tracking branch, such as `origin/main`, and
    /// nothing git would take it for first: a tag, or a local branch, of
    /// that name.
    pub(crate) fn is_remote_branch(&self, name: &str) -> Result<bool, Error> {
        Ok(self.has_ref(&format!("refs/remotes/{name}"))?
            && !self.has_ref(&format!("refs/heads/{name}"))?
            && !self.has_ref(&format!("refs/tags/{name}"))?)
    }

    /// The name a remote-tracking branch has on its remote: `feature/x` for
    /// `origin/feature/x`. Remote names may hold slashes; the longest
    /// configured one wins, and without one the first part is the remote's.
    pub(crate) fn name_on_its_remote(&self, remote_branch: &str) -> String {
        let remotes = self.remote_names();
        match crate::refs::remote_of(remote_branch, &remotes) {
            Some(remote) => remote_branch[remote.len() + 1..].to_owned(),
            None => remote_branch
                .split_once('/')
                .map_or(remote_branch, |(_, rest)| rest)
                .to_owned(),
        }
    }

    /// Whether the reference `full` is here. A name no reference could have -
    /// `refs/heads/HEAD~1`, typed where a branch was wanted - is not here.
    pub(crate) fn has_ref(&self, full: &str) -> Result<bool, Error> {
        if gix::refs::FullName::try_from(full).is_err() {
            return Ok(false);
        }
        Ok(self
            .inner
            .try_find_reference(full)
            .map_err(wrap(Error::References))?
            .is_some())
    }

    /// The commit the reference `full` finally names, if it is here and names
    /// one.
    fn peeled_commit(&self, full: &str) -> Result<Option<gix::ObjectId>, Error> {
        let Some(mut reference) = self
            .inner
            .try_find_reference(full)
            .map_err(wrap(Error::References))?
        else {
            return Ok(None);
        };
        Ok(reference
            .peel_to_id()
            .ok()
            .map(|id| id.detach())
            .filter(|id| self.is_commit(*id)))
    }

    /// Whether `id` is a commit: a tag may point at a tree or a blob, and a
    /// walk of the history cannot start from those.
    fn is_commit(&self, id: gix::ObjectId) -> bool {
        self.inner
            .find_header(id)
            .is_ok_and(|header| header.kind() == gix::object::Kind::Commit)
    }
}

/// The full name of the local branch `name`, or why it cannot be one.
fn full_branch(name: &str) -> Result<gix::refs::FullName, Error> {
    gix::refs::FullName::try_from(format!("refs/heads/{name}"))
        .map_err(|_| Error::Ref(format!("{name} cannot be the name of a branch")))
}
