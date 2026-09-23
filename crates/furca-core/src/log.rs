use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};

/// Where a history walk starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tips {
    /// From `HEAD` only, as `git log` does.
    Head,
    /// From `HEAD` and every branch, remote-tracking branch and tag, as
    /// `git log --all` does — the history a graph shows.
    All,
}

/// The first commits of a history walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Log {
    /// Commits with no parent listed before any of its children; otherwise
    /// newest commit time first (`git log --date-order`).
    pub commits: Vec<Commit>,
    /// Whether the walk stopped at the limit with more history behind it.
    pub truncated: bool,
}

/// One commit, with just enough to draw a graph row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Commit {
    /// The full hex object id.
    pub id: String,
    /// Parent ids in order; the first is the mainline.
    pub parents: Vec<String>,
    pub author: Person,
    pub committer: Person,
    /// The first paragraph of the message, folded to one line.
    pub subject: String,
}

/// Who made a commit and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub name: String,
    pub email: String,
    /// RFC 3339 with the person's own UTC offset, such as
    /// `2026-09-23T10:15:00+03:00`.
    pub time: String,
}

impl Repository {
    /// Walks the history from `tips` and returns at most `limit` commits.
    ///
    /// Parents always come after all of their children, so the result can be
    /// drawn as a graph top to bottom; within that, newer commits come first.
    /// A repository without commits yields an empty log, not an error.
    pub fn log(&self, tips: Tips, limit: usize) -> Result<Log, Error> {
        let starts = self.tips(tips)?;
        if starts.is_empty() || limit == 0 {
            return Ok(Log {
                truncated: !starts.is_empty(),
                commits: Vec::new(),
            });
        }

        let graph = self
            .inner
            .commit_graph_if_enabled()
            .map_err(wrap(Error::Walk))?;
        let walk = gix::traverse::commit::topo::Builder::from_iters(
            &self.inner.objects,
            starts,
            None::<Vec<gix::ObjectId>>,
        )
        .with_commit_graph(graph)
        .sorting(gix::traverse::commit::topo::Sorting::DateOrder)
        .build()
        .map_err(wrap(Error::Walk))?;

        let mut commits = Vec::with_capacity(limit.min(4096));
        let mut truncated = false;
        for info in walk {
            let info = info.map_err(wrap(Error::Walk))?;
            if commits.len() == limit {
                truncated = true;
                break;
            }
            commits.push(self.commit(info.id)?);
        }

        Ok(Log { commits, truncated })
    }

    /// The object ids a walk from `tips` starts at.
    fn tips(&self, tips: Tips) -> Result<Vec<gix::ObjectId>, Error> {
        let mut ids = Vec::new();
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        if let Some(id) = head.id() {
            ids.push(id.detach());
        }

        if tips == Tips::All {
            let platform = self.inner.references().map_err(wrap(Error::References))?;
            for reference in platform.all().map_err(wrap(Error::References))? {
                let mut reference = reference.map_err(Error::References)?;
                let Ok(id) = reference.peel_to_id() else {
                    continue;
                };
                // A tag may point at a tree or a blob; only commits start a walk.
                let is_commit = self
                    .inner
                    .find_header(id)
                    .is_ok_and(|header| header.kind() == gix::object::Kind::Commit);
                if is_commit {
                    ids.push(id.detach());
                }
            }
        }

        // Duplicates are left in: the topological walk skips a tip it has
        // already seen, and a second pass here would only repeat that work.
        Ok(ids)
    }

    fn commit(&self, id: gix::ObjectId) -> Result<Commit, Error> {
        let failed = |source: crate::error::Source| Error::Commit {
            id: id.to_string(),
            source,
        };
        let commit = self
            .inner
            .find_commit(id)
            .map_err(|e| failed(Box::new(e)))?;
        let decoded = commit.decode().map_err(|e| failed(Box::new(e)))?;

        Ok(Commit {
            id: id.to_string(),
            parents: decoded.parents().map(|parent| parent.to_string()).collect(),
            author: person(decoded.author().map_err(|e| failed(Box::new(e)))?),
            committer: person(decoded.committer().map_err(|e| failed(Box::new(e)))?),
            subject: decoded.message_summary().to_string(),
        })
    }
}

fn person(signature: gix::actor::SignatureRef<'_>) -> Person {
    let signature = signature.trim();
    // A malformed date is shown as the epoch rather than failing the whole
    // log: one bad commit from an old import must not hide the history.
    let time = signature
        .time()
        .unwrap_or_default()
        .format_or_unix(gix::date::time::format::ISO8601_STRICT);
    Person {
        name: signature.name.to_string(),
        email: signature.email.to_string(),
        time,
    }
}
