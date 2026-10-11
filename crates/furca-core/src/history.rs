use crate::Repository;
use crate::error::{Error, wrap};
use crate::log::Commit;

/// The commits reachable from `HEAD` that a base revision cannot reach, newest
/// committer time first - what `git log <base>..HEAD` lists.
///
/// Read one commit at a time: a caller looking for one commit (the last
/// dependency update, say) stops when it has it and leaves the rest of the
/// history unread. Unlike [`Repository::log`], the order is plain committer
/// time rather than the graph's: this is for reading what happened, not for
/// drawing it.
pub struct History<'repo> {
    repo: &'repo Repository,
    walk: Option<gix::revision::Walk<'repo>>,
}

impl Repository {
    /// The history of `HEAD` down to, and without, what `since` can reach.
    ///
    /// `since` is any revision git understands - a tag such as `v0.2.0`, a
    /// branch, an id. Without it the whole history is read. A repository
    /// without commits yields nothing.
    pub fn history(&self, since: Option<&str>) -> Result<History<'_>, Error> {
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        let Some(head) = head.id() else {
            return Ok(History {
                repo: self,
                walk: None,
            });
        };
        let mut hidden = Vec::new();
        if let Some(since) = since {
            hidden.push(self.resolve(since)?);
        }
        let walk = self
            .inner
            .rev_walk([head.detach()])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .with_hidden(hidden)
            .all()
            .map_err(wrap(Error::Walk))?;
        Ok(History {
            repo: self,
            walk: Some(walk),
        })
    }

    /// The commit a revision names - an id, a tag, a branch, anything git
    /// understands - with an annotated tag peeled through to its commit.
    ///
    /// For a reader that knows which commit it wants: the date a release tag
    /// was made on, the tip of one branch. A revision that names no commit
    /// (a tag on a tree, a name nothing has) is [`Error::Revision`].
    pub fn commit(&self, revision: &str) -> Result<Commit, Error> {
        let id = self.resolve(revision)?;
        self.read_commit(id)
    }

    /// The whole message of a commit as it was written: subject, body and
    /// trailers, with the trailing newline git adds trimmed.
    pub fn message(&self, id: &str) -> Result<String, Error> {
        let failed = |source: crate::error::Source| Error::Commit {
            id: id.to_owned(),
            source,
        };
        let object = gix::ObjectId::from_hex(id.as_bytes()).map_err(|e| failed(e.into()))?;
        let commit = self
            .inner
            .find_commit(object)
            .map_err(|e| failed(e.into()))?;
        let message = commit.message_raw().map_err(|e| failed(e.into()))?;
        Ok(message.to_string().trim_end().to_owned())
    }

    /// Whether `ancestor` is `descendant` or can be reached from it - what
    /// `git merge-base --is-ancestor` answers. Both are revisions git
    /// understands.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, Error> {
        let ancestor = self.resolve(ancestor)?;
        let descendant = self.resolve(descendant)?;
        if ancestor == descendant {
            return Ok(true);
        }
        // Unrelated histories have no merge base at all: an empty list, not
        // an error.
        let bases = self
            .inner
            .merge_bases_many(ancestor, &[descendant])
            .map_err(wrap(Error::Walk))?;
        Ok(bases.iter().any(|base| base.detach() == ancestor))
    }

    pub(crate) fn resolve(&self, revision: &str) -> Result<gix::ObjectId, Error> {
        let failed = |source: crate::error::Source| Error::Revision {
            revision: revision.to_owned(),
            source,
        };
        let id = self
            .inner
            .rev_parse_single(revision)
            .map_err(|e| failed(e.into()))?;
        let commit = id
            .object()
            .map_err(|e| failed(e.into()))?
            .peel_to_commit()
            .map_err(|e| failed(e.into()))?;
        Ok(commit.id)
    }
}

impl Iterator for History<'_> {
    type Item = Result<Commit, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let info = match self.walk.as_mut()?.next()? {
            Ok(info) => info,
            Err(error) => return Some(Err(Error::Walk(error.into()))),
        };
        Some(self.repo.read_commit(info.id))
    }
}
