//! Rebasing: the commits a rebase would replay, and what is to become of
//! each.
//!
//! git asks for an interactive rebase's plan in an editor. Here the plan is
//! read first ([`Repository::rebase_plan`]): the commits between where the
//! rebase lands and `HEAD`, oldest first, each with its whole message. What
//! is to become of each goes back as a list ([`RebaseTodo`]) that names every
//! one of them once, in the order to replay them - a commit left out by
//! mistake is not a commit dropped (ADR 0024).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::error::{Error, wrap};
use crate::log::Commit;

/// The commits a rebase would replay, as they stand now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RebasePlan {
    /// The branch `HEAD` is on, which the rebase rewrites; `None` on no
    /// branch.
    pub branch: Option<String>,
    /// The commit the steps are replayed on; `None` when the whole history
    /// is replayed, from its first commit.
    pub onto: Option<Commit>,
    /// The commits to replay, oldest first.
    pub steps: Vec<RebaseStep>,
}

/// One commit a rebase would replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RebaseStep {
    pub commit: Commit,
    /// Its whole message, as it was written.
    pub message: String,
    /// Whether the branch's upstream has it already: rewriting it parts the
    /// branch from what was pushed.
    pub pushed: bool,
}

/// What is to become of one commit of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct RebaseTodo {
    /// The commit: its id, or the start of it.
    pub commit: String,
    #[serde(flatten)]
    pub action: RebaseAction,
}

/// What a rebase does with a commit, in git's own words for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "action", rename_all = "kebab-case")]
pub enum RebaseAction {
    /// Replay it as it is.
    Pick,
    /// Replay it under another message.
    Reword { message: String },
    /// Fold it into the commit before it, their messages one after the
    /// other.
    Squash,
    /// Fold it into the commit before it, and leave its message out.
    Fixup,
    /// Leave it out.
    Drop,
}

impl Repository {
    /// The commits a rebase onto `onto` would replay - a branch, a
    /// remote-tracking branch, a tag, a commit - or, with `None`, every
    /// commit `HEAD` has, from the first.
    ///
    /// A merge among them is refused: a rebase would replay what it brought
    /// in as commits of the branch's own, and the merge would be gone.
    pub fn rebase_plan(&self, onto: Option<&str>) -> Result<RebasePlan, Error> {
        let head = self.head()?;
        let Some(tip) = self.inner.head_id().ok().map(|id| id.detach()) else {
            return Err(Error::Operation(
                "there is no commit here yet: nothing to rebase".to_owned(),
            ));
        };
        let onto = onto.map(|name| self.rebase_onto(name)).transpose()?;
        let ours = self.reachable(tip, onto)?;
        let pushed = match head.upstream.as_deref() {
            Some(upstream) => match self.resolve(&format!("refs/remotes/{upstream}")) {
                Ok(there) => Some(self.reachable(tip, Some(there))?),
                Err(_) => None,
            },
            None => None,
        };

        let mut steps = Vec::with_capacity(ours.len());
        let mut at = Some(tip);
        while let Some(id) = at.filter(|id| ours.contains(id)) {
            let commit = self.read_commit(id)?;
            if commit.parents.len() > 1 {
                return Err(Error::Operation(format!(
                    "{} is a merge: a rebase would replay what it brought in commit by commit and leave the merge out; rebase what comes after it",
                    &commit.id[..7]
                )));
            }
            at = match commit.parents.first() {
                Some(parent) => {
                    Some(
                        gix::ObjectId::from_hex(parent.as_bytes()).map_err(|e| Error::Commit {
                            id: parent.clone(),
                            source: e.into(),
                        })?,
                    )
                }
                None => None,
            };
            steps.push(RebaseStep {
                message: self.message(&commit.id)?,
                pushed: pushed.as_ref().is_some_and(|ahead| !ahead.contains(&id)),
                commit,
            });
        }
        steps.reverse();
        Ok(RebasePlan {
            branch: head.branch,
            onto: onto.map(|id| self.read_commit(id)).transpose()?,
            steps,
        })
    }

    /// The commits of the line `HEAD` came by that can be rewritten in
    /// place - squashed down to, or given a change to absorb - newest first,
    /// at most `limit` of them: down to the first merge, which stays as it
    /// is.
    pub fn rewritable(&self, limit: usize) -> Result<Vec<RebaseStep>, Error> {
        let head = self.head()?;
        let Some(tip) = self.inner.head_id().ok().map(|id| id.detach()) else {
            return Ok(Vec::new());
        };
        let ahead = match head.upstream.as_deref() {
            Some(upstream) => match self.resolve(&format!("refs/remotes/{upstream}")) {
                Ok(there) => Some(self.reachable(tip, Some(there))?),
                Err(_) => None,
            },
            None => None,
        };
        let mut steps = Vec::new();
        let mut at = Some(tip);
        while let Some(id) = at.filter(|_| steps.len() < limit) {
            let commit = self.read_commit(id)?;
            if commit.parents.len() > 1 {
                break;
            }
            at = commit
                .parents
                .first()
                .and_then(|parent| gix::ObjectId::from_hex(parent.as_bytes()).ok());
            steps.push(RebaseStep {
                message: self.message(&commit.id)?,
                pushed: ahead.as_ref().is_some_and(|ahead| !ahead.contains(&id)),
                commit,
            });
        }
        Ok(steps)
    }

    /// The commit a rebase lands on for `name`: a ref by its full name, or a
    /// local branch, a remote-tracking branch or a tag of that name - in that
    /// order, as a checkout looks for one - or else the commit `name`
    /// resolves to.
    pub(crate) fn rebase_onto(&self, name: &str) -> Result<gix::ObjectId, Error> {
        if name.starts_with("refs/") && self.has_ref(name)? {
            return self.resolve(name);
        }
        for kind in ["heads", "remotes", "tags"] {
            let full = format!("refs/{kind}/{name}");
            if self.has_ref(&full)? {
                return self.resolve(&full);
            }
        }
        self.resolve(name)
    }

    /// The commits `tip` has that `hidden` does not.
    fn reachable(
        &self,
        tip: gix::ObjectId,
        hidden: Option<gix::ObjectId>,
    ) -> Result<HashSet<gix::ObjectId>, Error> {
        let walk = self
            .inner
            .rev_walk([tip])
            .with_hidden(hidden)
            .all()
            .map_err(wrap(Error::Walk))?;
        walk.map(|info| info.map(|info| info.id).map_err(wrap(Error::Walk)))
            .collect()
    }
}
