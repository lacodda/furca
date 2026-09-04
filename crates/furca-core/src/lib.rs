//! furca-core: a plain library over gitoxide, with no Tauri or terminal
//! knowledge of its own. See `docs/adr/0002-one-core-three-doors.md`.

use std::path::Path;

use serde::Serialize;

/// A repository opened for reading.
///
/// Wraps a `gix::Repository` handle. Reads (log, status, diff, blame, refs)
/// happen in-process through this handle; mutations are out of scope here and
/// go through the system `git` CLI instead (ADR 0001).
pub struct Repository {
    inner: gix::Repository,
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
}

/// Errors returned by furca-core.
///
/// gix's own error types carry a lot of context and are boxed here so that a
/// `Result<T, Error>` stays cheap to move regardless of which variant it holds.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not open the repository: {0}")]
    Open(#[from] Box<gix::discover::Error>),
    #[error("could not read HEAD: {0}")]
    Head(#[from] Box<gix::reference::find::existing::Error>),
}

impl Repository {
    /// Opens the repository containing `path`, discovering it the way `git`
    /// itself would — walking up through parent directories.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let inner = gix::discover(path.as_ref()).map_err(Box::new)?;
        Ok(Self { inner })
    }

    /// Reads the current state of `HEAD`.
    pub fn head(&self) -> Result<HeadSummary, Error> {
        let head = self.inner.head().map_err(Box::new)?;
        let branch = head.referent_name().map(|name| name.shorten().to_string());
        let detached = head.is_detached();
        let commit = head.id().map(|id| id.to_string());

        Ok(HeadSummary {
            branch,
            commit,
            detached,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn head_of_a_freshly_initialized_repository_has_no_commit() {
        let dir = tempfile::tempdir().expect("temp dir");
        gix::init(dir.path()).expect("gix::init");

        let repo = Repository::open(dir.path()).expect("furca-core opens what gix just created");
        let head = repo
            .head()
            .expect("HEAD reads even before the first commit");

        assert_eq!(
            head.commit, None,
            "a brand-new repository has no commit yet"
        );
        assert!(
            !head.detached,
            "a fresh HEAD points at the default branch, not a commit"
        );
        assert!(
            head.branch.is_some(),
            "a fresh HEAD names the default branch"
        );
    }
}
