use std::collections::HashSet;

use serde::Serialize;

use crate::Repository;
use crate::error::Error;
use crate::log::Tips;

/// What the commit-graph file holds of the history (ADR 0010).
///
/// The graph is listed in an order that needs generation numbers to be found
/// without walking the whole history first, and the commit-graph file is
/// where they come from (ADR 0005). A commit the file does not hold counts as
/// infinitely high, so every commit made since the file was written is
/// explored before anything is listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CommitGraph {
    /// The repository turned it off (`core.commitGraph = false`): git does
    /// not read one there, and neither does furca.
    Off,
    /// There is none.
    Missing,
    /// It holds the history but for `outside` commits reachable from a ref,
    /// counted up to [`CommitGraph::BEHIND`] and no further.
    Covers { outside: usize },
}

impl CommitGraph {
    /// How many commits outside the file make it worth writing again. Fewer
    /// cost a walk a few milliseconds; a `git` run costs more than that.
    pub const BEHIND: usize = 1000;

    /// Whether git should be asked to write the file.
    pub fn worth_writing(self) -> bool {
        match self {
            CommitGraph::Off => false,
            CommitGraph::Missing => true,
            CommitGraph::Covers { outside } => outside >= Self::BEHIND,
        }
    }
}

impl Repository {
    /// Reads how much of the history the commit-graph file holds.
    ///
    /// The file is closed under parents - a commit in it has every ancestor
    /// in it too - so counting stops at the first commit it holds on every
    /// line of descent, and the cost is the commits outside, not the history.
    pub fn commit_graph(&self) -> Result<CommitGraph, Error> {
        let enabled = self
            .inner
            .config_snapshot()
            .boolean("core.commitGraph")
            .unwrap_or(true);
        if !enabled {
            return Ok(CommitGraph::Off);
        }
        let graph = match self.inner.commit_graph() {
            Ok(graph) => graph,
            Err(error) if error.is_not_found() => return Ok(CommitGraph::Missing),
            Err(error) => return Err(Error::Walk(error.into())),
        };

        let mut queue = self.tips(&Tips::All)?;
        let mut seen: HashSet<gix::ObjectId> = HashSet::new();
        let mut outside = 0;
        while let Some(id) = queue.pop() {
            if !seen.insert(id) || graph.lookup(id).is_some() {
                continue;
            }
            // A tag may point at a tree or a blob, which has no place in a
            // graph of commits and no parents to follow.
            let Ok(commit) = self.inner.find_commit(id) else {
                continue;
            };
            outside += 1;
            if outside >= CommitGraph::BEHIND {
                break;
            }
            queue.extend(commit.parent_ids().map(|parent| parent.detach()));
        }
        Ok(CommitGraph::Covers { outside })
    }
}
