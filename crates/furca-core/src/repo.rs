use std::path::Path;

use serde::Serialize;

use crate::error::{Error, wrap};
use crate::git_cmd::CommandLog;

/// A repository: read in-process, changed through the system `git`.
///
/// Wraps a `gix::Repository` handle. Reads (log, refs, the working tree's
/// changes, and later diff and blame) happen in-process through this handle;
/// mutations go through the system `git` CLI instead (ADR 0001), by way of
/// [`Repository::git`].
pub struct Repository {
    pub(crate) inner: gix::Repository,
    pub(crate) commands: CommandLog,
}

/// Room for the commits of one graph screen and then some; a commit object
/// is a few hundred bytes.
const OBJECT_CACHE_BYTES: usize = 4 * 1024 * 1024;

/// Where a branch goes when it is pushed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Upstream {
    /// The local branch, such as `main`.
    pub branch: String,
    /// The remote's name, such as `origin`.
    pub remote: String,
    /// The remote's address, as configured for fetching: it names the
    /// repository, wherever a `pushurl` sends the pushes.
    pub url: String,
    /// The branch on the remote, as a full name: `refs/heads/main`.
    pub merge: String,
    /// The remote-tracking branch that mirrors it here, such as
    /// `origin/main`, if the remote's fetch refspecs give one.
    pub tracking: Option<String>,
}

/// A summary of the repository's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HeadSummary {
    /// The branch `HEAD` points at, if it is not detached.
    pub branch: Option<String>,
    /// The commit `HEAD` resolves to, as a hex object id, if it has one yet.
    pub commit: Option<String>,
    /// Whether `HEAD` points directly at a commit rather than a branch.
    pub detached: bool,
    /// The remote-tracking branch the current branch follows, such as
    /// `origin/main`, if one is configured.
    pub upstream: Option<String>,
}

impl Repository {
    /// Opens the repository containing `path`, discovering it the way `git`
    /// itself would — walking up through parent directories.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let mut inner = gix::discover(path.as_ref()).map_err(wrap(Error::Open))?;
        // A history walk reads every commit twice: once to order it, once to
        // describe it. Without a cache the second read decompresses again.
        inner.object_cache_size_if_unset(OBJECT_CACHE_BYTES);
        Ok(Self {
            inner,
            commands: CommandLog::default(),
        })
    }

    /// The root of the working tree; `None` for a bare repository.
    pub fn workdir(&self) -> Option<&Path> {
        self.inner.workdir()
    }

    /// The `.git` directory.
    pub fn git_dir(&self) -> &Path {
        self.inner.git_dir()
    }

    /// Every path the index tracks, relative to the root of the working tree
    /// with `/` separators, in the index's own order. A repository without
    /// an index yet tracks nothing.
    pub fn tracked_paths(&self) -> Result<Vec<String>, Error> {
        let index = self.inner.index_or_empty().map_err(wrap(Error::Index))?;
        Ok(index
            .entries()
            .iter()
            .map(|entry| entry.path(&index).to_string())
            .collect())
    }

    /// Reads the current state of `HEAD`.
    ///
    /// A repository without commits is not an error: its `HEAD` names the
    /// default branch and has no commit.
    pub fn head(&self) -> Result<HeadSummary, Error> {
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        let upstream = head.referent_name().and_then(|name| self.upstream_of(name));

        Ok(HeadSummary {
            branch: head.referent_name().map(|name| name.shorten().to_string()),
            commit: head.id().map(|id| id.to_string()),
            detached: head.is_detached(),
            upstream,
        })
    }

    /// Where the current branch is pushed and fetched: its remote, the
    /// remote's address and the branch there. `None` when `HEAD` is detached
    /// or the branch follows nothing - a release has nowhere to go then.
    pub fn upstream(&self) -> Result<Option<Upstream>, Error> {
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        let Some(branch) = head.referent_name() else {
            return Ok(None);
        };
        let short = branch.shorten();
        let Some(remote) = self
            .inner
            .branch_remote_name(short, gix::remote::Direction::Fetch)
        else {
            return Ok(None);
        };
        let remote = remote.as_bstr().to_string();
        let Some(merge) = self
            .inner
            .branch_remote_ref_name(branch, gix::remote::Direction::Fetch)
            .and_then(Result::ok)
        else {
            return Ok(None);
        };
        let url = self
            .inner
            .try_find_remote(remote.as_str())
            .and_then(Result::ok)
            .and_then(|r| {
                r.url(gix::remote::Direction::Push)
                    .map(|u| u.to_bstring().to_string())
            })
            // A branch may name a URL instead of a configured remote.
            .unwrap_or_else(|| remote.clone());
        Ok(Some(Upstream {
            branch: short.to_string(),
            remote,
            url,
            merge: merge.as_bstr().to_string(),
            tracking: self.upstream_of(branch),
        }))
    }

    /// The short name of the remote-tracking branch `branch` fetches into, if
    /// configured. A misconfigured upstream reads as none: it is a fact about
    /// the configuration, not a reason to fail the whole read.
    pub(crate) fn upstream_of(&self, branch: &gix::refs::FullNameRef) -> Option<String> {
        self.inner
            .branch_remote_tracking_ref_name(branch, gix::remote::Direction::Fetch)?
            .ok()
            .map(|name| name.shorten().to_string())
    }

    /// The packed-refs file, read once per listing. Peeling through it takes
    /// the peeled id `git pack-refs` already recorded for each annotated tag
    /// instead of opening every tag object — an eighth of the time on a
    /// repository with a few hundred refs.
    pub(crate) fn packed_refs(
        &self,
    ) -> Result<Option<gix::refs::file::packed::SharedBufferSnapshot>, Error> {
        self.inner
            .refs
            .cached_packed_buffer()
            .map_err(wrap(Error::References))
    }
}
