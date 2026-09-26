//! The order a graph is drawn in: a lazy topological walk.
//!
//! A commit may be listed once every child of it that the walk can reach has
//! been listed; among the commits that may be listed, the one with the newest
//! committer date goes first. That is `git log --date-order`.
//!
//! Knowing that a commit has no unlisted children means having looked at every
//! reachable commit that could be one. Generation numbers bound that: a child
//! always has a higher generation than its parent, so looking at every
//! reachable commit with a generation at or above a commit's own is enough.
//! The walk therefore explores in falling generation and only as deep as the
//! next commit to list requires — the first 500 commits of a 100 000-commit
//! history touch a few thousand, not all of them.
//!
//! Generation numbers come from the repository's commit-graph file. A commit
//! the file does not hold (made after the file was last written, or every
//! commit when there is no file) counts as infinitely high, as it does in git:
//! correct, but the walk then has to explore all such commits before it can
//! list any of them. See ADR 0004 for the order and ADR 0005 for the walk.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use gix::ObjectId;

use crate::error::{Error, Source};

/// The generation of a commit the commit-graph does not hold.
const INFINITE: u32 = u32::MAX;

struct Node {
    id: ObjectId,
    generation: u32,
    /// Committer time in seconds; what orders commits that may be listed.
    time: i64,
    /// Filled in when the node is explored.
    parents: Vec<u32>,
    /// Edges from explored children not yet listed.
    indegree: u32,
    queued: bool,
    listed: bool,
}

pub(crate) struct Walk<'repo> {
    repo: &'repo gix::Repository,
    graph: Option<gix::commitgraph::Graph>,
    nodes: Vec<Node>,
    index: gix::hashtable::HashMap<ObjectId, u32>,
    /// Nodes waiting to be explored, highest generation first.
    explore: BinaryHeap<(u32, u32)>,
    /// Nodes that may be listed, newest first; ties go to the one queued
    /// first. Entries go stale when a node gains a child or is listed, and
    /// are checked when popped.
    ready: BinaryHeap<(i64, Reverse<u64>, u32)>,
    sequence: u64,
    listed: usize,
}

impl<'repo> Walk<'repo> {
    pub(crate) fn new(
        repo: &'repo gix::Repository,
        graph: Option<gix::commitgraph::Graph>,
    ) -> Self {
        Self {
            repo,
            graph,
            nodes: Vec::new(),
            index: Default::default(),
            explore: BinaryHeap::new(),
            ready: BinaryHeap::new(),
            sequence: 0,
            listed: 0,
        }
    }

    /// Adds a starting point. A tip that is not a commit — a tag on a tree or
    /// a blob — is not a place a history starts, and is skipped.
    pub(crate) fn start(&mut self, id: ObjectId) -> Result<(), Error> {
        let Some(node) = self.node(id)? else {
            return Ok(());
        };
        self.queue_explore(node);
        self.push_ready(node);
        Ok(())
    }

    /// Whether commits remain that have not been listed.
    pub(crate) fn has_more(&self) -> bool {
        self.listed < self.nodes.len()
    }

    /// The next commit to list, or `None` once the history is exhausted.
    pub(crate) fn next_id(&mut self) -> Result<Option<ObjectId>, Error> {
        while let Some((_, _, node)) = self.ready.pop() {
            let at = node as usize;
            if self.nodes[at].listed {
                continue;
            }
            self.explore_down_to(self.nodes[at].generation)?;
            if self.nodes[at].indegree > 0 {
                // A child turned up; the node comes back when that child
                // has been listed.
                continue;
            }

            self.nodes[at].listed = true;
            self.listed += 1;
            for parent in std::mem::take(&mut self.nodes[at].parents) {
                let entry = &mut self.nodes[parent as usize];
                entry.indegree -= 1;
                if entry.indegree == 0 {
                    self.push_ready(parent);
                }
            }
            return Ok(Some(self.nodes[at].id));
        }
        Ok(None)
    }

    /// Explores every queued node whose generation is at least `generation`,
    /// counting its edges and queueing its parents.
    fn explore_down_to(&mut self, generation: u32) -> Result<(), Error> {
        while let Some(&(top, node)) = self.explore.peek() {
            if top < generation {
                break;
            }
            self.explore.pop();
            let parents = self.parent_ids(node)?;
            let mut indices = Vec::with_capacity(parents.len());
            for id in parents {
                // A parent that is not in the repository is the edge of a
                // shallow clone: the history ends there, as it does for git.
                let Some(parent) = self.node(id)? else {
                    continue;
                };
                self.nodes[parent as usize].indegree += 1;
                self.queue_explore(parent);
                indices.push(parent);
            }
            let entry = &mut self.nodes[node as usize];
            entry.parents = indices;
        }
        Ok(())
    }

    fn queue_explore(&mut self, node: u32) {
        let entry = &mut self.nodes[node as usize];
        if !entry.queued {
            entry.queued = true;
            self.explore.push((entry.generation, node));
        }
    }

    fn push_ready(&mut self, node: u32) {
        self.sequence += 1;
        let time = self.nodes[node as usize].time;
        self.ready.push((time, Reverse(self.sequence), node));
    }

    /// The node for `id`, created on first sight. `None` when `id` is not a
    /// commit in this repository.
    fn node(&mut self, id: ObjectId) -> Result<Option<u32>, Error> {
        if let Some(&node) = self.index.get(&id) {
            return Ok(Some(node));
        }

        let (generation, time) = match self.graph.as_ref().and_then(|g| g.commit_by_id(id)) {
            Some(commit) => (
                commit.generation(),
                i64::try_from(commit.committer_timestamp()).unwrap_or(i64::MAX),
            ),
            None => {
                let object = match self.repo.try_find_object(id) {
                    Ok(Some(object)) => object,
                    Ok(None) => return Ok(None),
                    Err(error) => return Err(walk_error(error)),
                };
                let Ok(commit) = object.try_into_commit() else {
                    return Ok(None);
                };
                let time = commit.time().map_err(walk_error)?.seconds;
                (INFINITE, time)
            }
        };

        let node = u32::try_from(self.nodes.len()).expect("fewer than 4 billion commits");
        self.nodes.push(Node {
            id,
            generation,
            time,
            parents: Vec::new(),
            indegree: 0,
            queued: false,
            listed: false,
        });
        self.index.insert(id, node);
        Ok(Some(node))
    }

    fn parent_ids(&self, node: u32) -> Result<Vec<ObjectId>, Error> {
        let id = self.nodes[node as usize].id;
        if let Some(graph) = &self.graph
            && let Some(commit) = graph.commit_by_id(id)
        {
            return commit
                .iter_parents()
                .map(|parent| {
                    parent
                        .map(|position| graph.id_at(position).to_owned())
                        .map_err(walk_error)
                })
                .collect();
        }
        let commit = self.repo.find_commit(id).map_err(walk_error)?;
        Ok(commit.parent_ids().map(|parent| parent.detach()).collect())
    }
}

fn walk_error(error: impl Into<Source>) -> Error {
    Error::Walk(error.into())
}
