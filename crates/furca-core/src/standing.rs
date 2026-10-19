use std::collections::BTreeSet;

use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};
use crate::log::{Commit, moment};

/// Where a repository stands: what it holds that has not gone anywhere yet,
/// and how far it is from its last tag.
///
/// Read from this machine alone, with no fetch: `behind` counts what the
/// last fetch brought, not what the remote has now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standing {
    /// The branch `HEAD` is on; `None` when it is detached.
    pub branch: Option<String>,
    /// The commit `HEAD` resolves to; `None` before the first commit.
    pub commit: Option<Commit>,
    /// The current branch against the remote-tracking branch it follows;
    /// `None` when it follows nothing.
    pub upstream: Option<Tracking>,
    /// Tracked paths whose content differs from `HEAD`, staged or not.
    pub changed: usize,
    /// What `git status` lists as untracked: files, and directories that
    /// hold nothing tracked counted once each.
    pub untracked: usize,
    /// Commits reachable from `HEAD` or a local branch that no
    /// remote-tracking branch reaches - what `git log --branches HEAD
    /// --not --remotes` lists. Without remotes, the whole history.
    pub unpushed: usize,
    /// Entries in the stash.
    pub stashes: usize,
    /// The nearest tag `HEAD` descends from, as `git describe --tags`
    /// picks it; `None` when no tag is in its history.
    pub tag: Option<NearestTag>,
    /// An operation git started and has not finished.
    pub operation: Option<Operation>,
}

/// The current branch against the remote-tracking branch it follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tracking {
    /// The remote-tracking branch, such as `origin/main`.
    pub name: String,
    /// Commits the branch has that the remote-tracking branch does not;
    /// `None` when that branch is gone - deleted on the remote and pruned.
    pub ahead: Option<usize>,
    /// Commits the remote-tracking branch has that the branch does not;
    /// `None` when it is gone.
    pub behind: Option<usize>,
}

/// A tag, and what `HEAD` has added since it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NearestTag {
    /// The short name, such as `v1.2.0`.
    pub name: String,
    /// Commits `HEAD` has that the tag does not: what has not been
    /// released yet.
    pub since: usize,
    /// When the tag was made, RFC 3339: the tagger's time for an annotated
    /// tag, the commit's for a plain one.
    pub time: String,
    /// The same moment as seconds since the Unix epoch.
    pub seconds: i64,
}

/// An operation git leaves halfway until it is continued or aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    /// `git am`, applying patches from a mailbox.
    Am,
}

impl Standing {
    /// Whether nothing here exists on this machine alone: no change left
    /// uncommitted, no untracked file, no commit unpushed, nothing stashed
    /// and no operation halfway. Being behind, or having commits since the
    /// last tag, does not unsettle a repository: nothing is lost if this
    /// machine is.
    pub fn settled(&self) -> bool {
        self.changed == 0
            && self.untracked == 0
            && self.unpushed == 0
            && self.stashes == 0
            && self.operation.is_none()
    }
}

impl Repository {
    /// Reads where the repository stands. A repository without commits is
    /// a standing too: nothing committed, nothing to push.
    pub fn standing(&self) -> Result<Standing, Error> {
        let head = self.head()?;
        let commit = head
            .commit
            .as_deref()
            .map(|id| self.commit(id))
            .transpose()?;
        let head_id = self.inner.head_id().ok().map(|id| id.detach());
        let (changed, untracked) = self.uncommitted()?;

        let upstream = match head.upstream {
            None => None,
            Some(name) => Some(self.tracking(name, head_id)?),
        };

        let mut local: Vec<gix::ObjectId> = head_id.into_iter().collect();
        local.extend(self.peeled(Kind::LocalBranches)?);
        let remote = self.peeled(Kind::RemoteBranches)?;

        Ok(Standing {
            branch: head.branch,
            commit,
            upstream,
            changed,
            untracked,
            unpushed: self.count(local, remote)?,
            stashes: self.stashes()?,
            tag: head_id
                .map(|id| self.nearest_tag(id))
                .transpose()?
                .flatten(),
            operation: self.inner.state().map(operation),
        })
    }

    /// Tracked paths that differ from `HEAD`, and untracked entries, counted
    /// as `git status` lists them. A bare repository has neither.
    fn uncommitted(&self) -> Result<(usize, usize), Error> {
        if self.workdir().is_none() {
            return Ok((0, 0));
        }
        let iter = self
            .inner
            .status(gix::progress::Discard)
            .map_err(wrap(Error::Status))?
            .untracked_files(gix::status::UntrackedFiles::Collapsed)
            .index_worktree_rewrites(None)
            .index_worktree_submodules(gix::status::Submodule::AsConfigured { check_dirty: true })
            .into_iter(Vec::new())
            .map_err(wrap(Error::Status))?;
        // A path both staged and changed again is one change, as in
        // `git status --short`.
        let mut changed = BTreeSet::new();
        let mut untracked = 0;
        for item in iter {
            let item = item.map_err(wrap(Error::Status))?;
            match &item {
                gix::status::Item::IndexWorktree(change) => match change.summary() {
                    None => {}
                    Some(gix::status::plumbing::index_as_worktree_with_renames::Summary::Added) => {
                        untracked += 1;
                    }
                    Some(_) => {
                        changed.insert(item.location().to_owned());
                    }
                },
                gix::status::Item::TreeIndex(_) => {
                    changed.insert(item.location().to_owned());
                }
            }
        }
        Ok((changed.len(), untracked))
    }

    /// The current branch against the remote-tracking branch `name`.
    fn tracking(&self, name: String, head: Option<gix::ObjectId>) -> Result<Tracking, Error> {
        let full = format!("refs/remotes/{name}");
        let tip = match self
            .inner
            .try_find_reference(full.as_str())
            .map_err(wrap(Error::References))?
        {
            Some(mut reference) => reference.peel_to_id().ok().map(|id| id.detach()),
            None => None,
        };
        let Some(tip) = tip else {
            return Ok(Tracking {
                name,
                ahead: None,
                behind: None,
            });
        };
        let head: Vec<gix::ObjectId> = head.into_iter().collect();
        Ok(Tracking {
            ahead: Some(self.count(head.clone(), vec![tip])?),
            behind: Some(self.count(vec![tip], head)?),
            name,
        })
    }

    /// The commits every branch of one kind points at. A branch pointing at
    /// a missing object, in a damaged or shallow clone, is left out, as
    /// [`Repository::refs`] leaves it out.
    fn peeled(&self, kind: Kind) -> Result<Vec<gix::ObjectId>, Error> {
        let platform = self.inner.references().map_err(wrap(Error::References))?;
        let packed = self.packed_refs()?;
        let packed = packed.as_ref().map(|buffer| &***buffer);
        let iter = match kind {
            Kind::LocalBranches => platform.local_branches(),
            Kind::RemoteBranches => platform.remote_branches(),
        }
        .map_err(wrap(Error::References))?;
        let mut ids = Vec::new();
        for reference in iter {
            let mut reference = reference.map_err(wrap(Error::References))?;
            if let Ok(id) = reference.peel_to_id_packed(packed) {
                ids.push(id.detach());
            }
        }
        Ok(ids)
    }

    /// How many commits `tips` reach that `hidden` do not.
    fn count(&self, tips: Vec<gix::ObjectId>, hidden: Vec<gix::ObjectId>) -> Result<usize, Error> {
        if tips.is_empty() {
            return Ok(0);
        }
        let walk = self
            .inner
            .rev_walk(tips)
            .with_hidden(hidden)
            .all()
            .map_err(wrap(Error::Walk))?;
        let mut count = 0;
        for info in walk {
            info.map_err(wrap(Error::Walk))?;
            count += 1;
        }
        Ok(count)
    }

    fn stashes(&self) -> Result<usize, Error> {
        let Some(stash) = self
            .inner
            .try_find_reference("refs/stash")
            .map_err(wrap(Error::References))?
        else {
            return Ok(0);
        };
        let mut log = stash.log_iter();
        let lines = log.all().map_err(wrap(Error::References))?;
        Ok(lines.map_or(0, Iterator::count))
    }

    /// The tag `git describe --tags` would name for `head`, with the commits
    /// since it counted exactly rather than estimated.
    fn nearest_tag(&self, head: gix::ObjectId) -> Result<Option<NearestTag>, Error> {
        let commit = self.inner.find_commit(head).map_err(wrap(Error::Walk))?;
        let Some(resolution) = commit
            .describe()
            .names(gix::commit::describe::SelectRef::AllTags)
            .try_resolve()
            .map_err(wrap(Error::Walk))?
        else {
            return Ok(None);
        };
        let Some(name) = resolution.outcome.name else {
            return Ok(None);
        };
        let name = name.to_string();
        let full = format!("refs/tags/{name}");
        let mut reference = self
            .inner
            .find_reference(full.as_str())
            .map_err(wrap(Error::References))?;
        // Peeling replaces the reference's target with what it peels to, so
        // the tag object, if there is one, is taken first.
        let object = reference.target().try_id().map(ToOwned::to_owned);
        let target = reference
            .peel_to_id()
            .map_err(wrap(Error::References))?
            .detach();
        let (time, seconds) = self.tag_time(object, target)?;
        Ok(Some(NearestTag {
            since: self.count(vec![head], vec![target])?,
            name,
            time,
            seconds,
        }))
    }

    /// When a tag was made: the tagger's time if `object`, what the tag
    /// reference names, is a tag object, the commit's otherwise.
    fn tag_time(
        &self,
        object: Option<gix::ObjectId>,
        commit: gix::ObjectId,
    ) -> Result<(String, i64), Error> {
        let tagger = object
            .and_then(|id| self.inner.find_object(id).ok())
            .and_then(|object| object.try_into_tag().ok())
            .and_then(|tag| {
                let tagger = tag.tagger().ok().flatten()?;
                tagger.time().ok()
            });
        if let Some(time) = tagger {
            return Ok(moment(time));
        }
        let commit = self.commit(&commit.to_string())?;
        Ok((commit.committer.time, commit.committer.seconds))
    }
}

enum Kind {
    LocalBranches,
    RemoteBranches,
}

fn operation(state: gix::state::InProgress) -> Operation {
    use gix::state::InProgress;
    match state {
        InProgress::Merge => Operation::Merge,
        InProgress::Rebase | InProgress::RebaseInteractive | InProgress::ApplyMailboxRebase => {
            Operation::Rebase
        }
        InProgress::CherryPick | InProgress::CherryPickSequence => Operation::CherryPick,
        InProgress::Revert | InProgress::RevertSequence => Operation::Revert,
        InProgress::Bisect => Operation::Bisect,
        InProgress::ApplyMailbox => Operation::Am,
    }
}
