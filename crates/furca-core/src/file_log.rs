use serde::Serialize;

use crate::Repository;
use crate::diff::FileStatus;
use crate::error::{Error, Source, wrap};
use crate::log::Commit;

/// The commits that changed one file, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileLog {
    pub commits: Vec<FileRevision>,
    /// Whether the walk stopped at the limit with more history behind it.
    pub truncated: bool,
}

/// A commit that changed the file, and the path the file had there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileRevision {
    pub commit: Commit,
    /// The file's path in this commit - for a deletion, the path it had -
    /// which is what [`Repository::file_at`] reads it by.
    pub path: String,
    pub status: FileStatus,
    /// Where the file was before, when this commit moved it.
    pub from: Option<String>,
}

impl Repository {
    /// The commits that changed the file at `path`, from `HEAD` down its
    /// first parents, newest first, at most `limit` of them.
    ///
    /// A merge counts when it changed the file against the line it was
    /// merged into. A rename is followed, as `git log --follow` does: past
    /// the commit that moved the file, its history goes on under the old
    /// path. A path that is not there now still has a history if it was
    /// there once.
    pub fn file_log(&self, path: &str, limit: usize) -> Result<FileLog, Error> {
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        let Some(head) = head.id() else {
            return Ok(FileLog {
                commits: Vec::new(),
                truncated: false,
            });
        };
        let mut id = head.detach();
        let mut path = path.to_owned();
        let mut current = self.entry_at(id, &path)?;
        let mut commits = Vec::new();
        loop {
            let parent = self.first_parent(id)?;
            let mut previous = match parent {
                Some(parent) => self.entry_at(parent, &path)?,
                None => None,
            };
            if current != previous {
                if commits.len() == limit {
                    return Ok(FileLog {
                        commits,
                        truncated: true,
                    });
                }
                let moved_from = match (parent, &previous, &current) {
                    (Some(parent), None, Some(_)) => self.renamed_from(parent, id, &path)?,
                    _ => None,
                };
                let status = match (&previous, &current, &moved_from) {
                    (_, _, Some(_)) => FileStatus::Renamed,
                    (None, _, None) => FileStatus::Added,
                    (_, None, None) => FileStatus::Deleted,
                    _ => FileStatus::Modified,
                };
                commits.push(FileRevision {
                    commit: self.read_commit(id)?,
                    path: path.clone(),
                    status,
                    from: moved_from.clone(),
                });
                if let (Some(parent), Some(from)) = (parent, moved_from) {
                    previous = self.entry_at(parent, &from)?;
                    path = from;
                }
            }
            match parent {
                Some(parent) => {
                    id = parent;
                    current = previous;
                }
                None => {
                    return Ok(FileLog {
                        commits,
                        truncated: false,
                    });
                }
            }
        }
    }

    fn first_parent(&self, id: gix::ObjectId) -> Result<Option<gix::ObjectId>, Error> {
        let commit = self.inner.find_commit(id).map_err(|e| Error::Commit {
            id: id.to_string(),
            source: e.into(),
        })?;
        Ok(commit.parent_ids().next().map(|parent| parent.detach()))
    }

    /// The path the file at `path` in `commit` was moved from, when
    /// `commit` renamed it rather than adding it.
    fn renamed_from(
        &self,
        parent: gix::ObjectId,
        commit: gix::ObjectId,
        path: &str,
    ) -> Result<Option<String>, Error> {
        let failed = |source: Source| Error::Changes {
            commit: commit.to_string(),
            source,
        };
        let tree = |id: gix::ObjectId| {
            self.inner
                .find_commit(id)
                .and_then(|commit| commit.tree())
                .map_err(|e| failed(e.into()))
        };
        let (before, after) = (tree(parent)?, tree(commit)?);
        let changes = self
            .inner
            .diff_tree_to_tree(Some(&before), Some(&after), None)
            .map_err(|e| failed(e.into()))?;
        Ok(changes.into_iter().find_map(|change| match change {
            gix::object::tree::diff::ChangeDetached::Rewrite {
                source_location,
                location,
                copy: false,
                ..
            } if location == path.as_bytes() => Some(source_location.to_string()),
            _ => None,
        }))
    }
}
