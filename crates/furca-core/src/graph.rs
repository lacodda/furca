//! The graph a history is drawn as: every commit in a lane of its own, and
//! the lines that join it to its children above and to its parents below.
//!
//! Lanes are laid out in the order the graph is listed (ADR 0004), one row
//! per commit, top to bottom. A lane waits for one commit: the parent that
//! the commit above it named. When that commit's row comes, every lane
//! waiting for it ends there, and the commit continues the leftmost of them
//! down to its own first parent; each further parent of a merge joins a lane
//! that already waits for it, or opens one of its own. A commit nothing
//! waits for - the tip of a branch - opens a lane in the leftmost free
//! column. So the line of a branch keeps its column and its colour from its
//! tip down to where it forks off, which is what lets the eye follow it.
//!
//! What is kept of a row is only what the commit changes: its column and
//! colour, the lanes that end at it and the lanes it leads down into. Most of
//! a wide graph is lanes passing by, and those follow from the lanes above
//! the row - so instead of keeping them, the state of the lanes is kept every
//! [`EVERY`] rows, and a window of rows is drawn by playing the changes
//! forward from the mark before it. A history of 100 000 commits keeps a few
//! megabytes, however many branches run beside each other.

use std::collections::HashSet;
use std::ops::Range;

use gix::ObjectId;
use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};
use crate::log::{Commit, Tips};
use crate::walk::Walk;

/// How many colours the lanes of a graph take turns with: the series colours
/// of the line's theme.
pub const COLORS: u8 = 8;

/// How many rows apart the state of the lanes is kept.
const EVERY: usize = 64;

/// One row of a graph as it is drawn: the commit's place, and every lane
/// that crosses the row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Row {
    /// The lane the commit sits in, counted from the left from 0.
    pub column: u16,
    /// The colour of that lane, below [`COLORS`].
    pub color: u8,
    /// The lanes coming down into the row from the left, then the segments
    /// from the commit down to its parents, in the order the commit names
    /// them.
    pub segments: Vec<Segment>,
}

/// The part of one lane that crosses a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Segment {
    pub kind: SegmentKind,
    /// The lane the segment starts in: at the top of the row for `pass` and
    /// `in`, the commit's own for `out`.
    pub from: u16,
    /// The lane it ends in: at the bottom of the row for `pass` and `out`,
    /// the commit's own for `in`.
    pub to: u16,
    /// The colour of the lane, below [`COLORS`].
    pub color: u8,
}

/// What a segment does in its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum SegmentKind {
    /// A lane passing the commit by, top to bottom, in one column.
    Pass,
    /// A lane from a child above, ending at the commit.
    In,
    /// From the commit down into the lane that leads to one of its parents.
    Out,
}

/// The graph of a history, laid out as far as it has been asked to be.
///
/// It owns no repository: [`Graph::extend`] is handed one, so a graph can be
/// kept and laid out a chunk at a time, on whichever thread asks next.
pub struct Graph {
    tips: Tips,
    /// The commits the walk started from, sorted: what the refs pointed at
    /// when the graph was begun.
    starts: Vec<ObjectId>,
    /// `None` once the history is laid out to its end.
    walk: Option<Walk>,
    lanes: Lanes,
    ids: Vec<ObjectId>,
    rows: Vec<Stored>,
    changes: Vec<Change>,
    /// The colour of every lane at the top of row `n * EVERY`, `None` where a
    /// column is free.
    marks: Vec<Vec<Option<u8>>>,
    width: u16,
}

/// What is kept of a row.
#[derive(Clone, Copy)]
struct Stored {
    column: u16,
    color: u8,
    /// Where the row's changes end in [`Graph::changes`].
    end: u32,
}

/// A lane ending at the commit (`out` false: the column it comes down in),
/// or one the commit leads down into (`out` true: its column and colour).
#[derive(Clone, Copy)]
struct Change {
    out: bool,
    column: u16,
    color: u8,
}

impl Repository {
    /// Begins the graph of the history from `tips`. Nothing is laid out
    /// until [`Graph::extend`] asks for rows.
    pub fn graph(&self, tips: Tips) -> Result<Graph, Error> {
        let mut starts = self.tips(&tips)?;
        let commit_graph = self
            .inner
            .commit_graph_if_enabled()
            .map_err(wrap(Error::Walk))?;
        let mut walk = Walk::new(commit_graph);
        for &id in &starts {
            walk.start(&self.inner, id)?;
        }
        starts.sort_unstable();
        starts.dedup();
        let more = walk.has_more();
        Ok(Graph {
            tips,
            starts,
            walk: more.then_some(walk),
            lanes: Lanes::default(),
            ids: Vec::new(),
            rows: Vec::new(),
            changes: Vec::new(),
            marks: Vec::new(),
            width: 0,
        })
    }

    /// Whether `graph` still shows the history the refs lead to: false once
    /// a commit, a fetch, a new branch or a moved tag changed what it starts
    /// from, or a revision it was begun from names nothing any more.
    pub fn graph_is_current(&self, graph: &Graph) -> Result<bool, Error> {
        let mut starts = match self.tips(&graph.tips) {
            Ok(starts) => starts,
            Err(Error::Revision { .. }) => return Ok(false),
            Err(error) => return Err(error),
        };
        starts.sort_unstable();
        starts.dedup();
        Ok(starts == graph.starts)
    }
}

impl Graph {
    /// Lays out up to `rows` more rows and says how many it added: fewer
    /// once the history runs out.
    pub fn extend(&mut self, repo: &Repository, rows: usize) -> Result<usize, Error> {
        let Some(walk) = self.walk.as_mut() else {
            return Ok(0);
        };
        let mut added = 0;
        while added < rows {
            let Some(id) = walk.next_id(&repo.inner)? else {
                break;
            };
            if self.ids.len().is_multiple_of(EVERY) {
                self.marks.push(self.lanes.colors());
            }
            let (column, color) = self.lanes.place(id, walk.parents(), &mut self.changes);
            self.ids.push(id);
            self.rows.push(Stored {
                column,
                color,
                end: u32::try_from(self.changes.len()).expect("fewer than 4 billion lines"),
            });
            self.width = self.width.max(self.lanes.width()).max(column + 1);
            added += 1;
        }
        if !walk.has_more() {
            self.walk = None;
        }
        Ok(added)
    }

    /// How many rows are laid out.
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// Whether no row is laid out yet.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Whether the whole history is laid out.
    pub fn is_complete(&self) -> bool {
        self.walk.is_none()
    }

    /// The most lanes any row laid out so far needs side by side.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Where the graph starts: what it was begun with.
    pub fn tips(&self) -> &Tips {
        &self.tips
    }

    /// The rows in `range`, as far as they are laid out.
    pub fn rows(&self, range: Range<usize>) -> Vec<Row> {
        let end = range.end.min(self.ids.len());
        if range.start >= end {
            return Vec::new();
        }
        let mark = range.start / EVERY;
        let mut lanes = self.marks[mark].clone();
        let mut out = Vec::with_capacity(end - range.start);
        for at in mark * EVERY..end {
            let row = self.replay(at, &mut lanes);
            if at >= range.start {
                out.push(row);
            }
        }
        out
    }

    /// The full hex id of the commit at row `at`.
    pub fn id(&self, at: usize) -> Option<String> {
        self.ids.get(at).map(ToString::to_string)
    }

    /// The row of the commit `id` (full hex), if it is laid out.
    pub fn position(&self, id: &str) -> Option<usize> {
        let id = ObjectId::from_hex(id.as_bytes()).ok()?;
        self.ids.iter().position(|candidate| *candidate == id)
    }

    /// The rows of the commits `ids` (full hex), in order; an id the graph
    /// does not hold is left out.
    pub fn positions(&self, ids: &[String]) -> Vec<usize> {
        let wanted: HashSet<ObjectId> = ids
            .iter()
            .filter_map(|id| ObjectId::from_hex(id.as_bytes()).ok())
            .collect();
        self.ids
            .iter()
            .enumerate()
            .filter(|(_, id)| wanted.contains(*id))
            .map(|(at, _)| at)
            .collect()
    }

    /// The commits of the rows in `range`, as far as they are laid out.
    pub fn commits(&self, repo: &Repository, range: Range<usize>) -> Result<Vec<Commit>, Error> {
        let end = range.end.min(self.ids.len());
        let start = range.start.min(end);
        self.ids[start..end]
            .iter()
            .map(|&id| repo.read_commit(id))
            .collect()
    }

    /// The rows in `range` whose commit `query` finds, in order.
    pub fn find(
        &self,
        repo: &Repository,
        query: &Query,
        range: Range<usize>,
    ) -> Result<Vec<usize>, Error> {
        let end = range.end.min(self.ids.len());
        let start = range.start.min(end);
        let mut hex = gix::hash::Kind::hex_buf();
        let mut found = Vec::new();
        for (at, &id) in self.ids[start..end].iter().enumerate() {
            let failed = |source: crate::error::Source| Error::Commit {
                id: id.to_string(),
                source,
            };
            let commit = repo.inner.find_commit(id).map_err(|e| failed(e.into()))?;
            let decoded = commit.decode().map_err(|e| failed(e.into()))?;
            let author = decoded.author().map_err(|e| failed(e.into()))?;
            let written = id.hex_to_buf(&mut hex);
            if query.finds(
                written.as_bytes(),
                &decoded.message_summary(),
                author.name,
                author.email,
            ) {
                found.push(start + at);
            }
        }
        Ok(found)
    }

    /// Draws row `at` from the lanes above it, and leaves the lanes below.
    fn replay(&self, at: usize, lanes: &mut Vec<Option<u8>>) -> Row {
        let stored = self.rows[at];
        let begin = if at == 0 {
            0
        } else {
            self.rows[at - 1].end as usize
        };
        let changes = &self.changes[begin..stored.end as usize];
        let ends = |column: usize| {
            changes
                .iter()
                .any(|change| !change.out && usize::from(change.column) == column)
        };

        let mut segments = Vec::with_capacity(lanes.len() + 2);
        for (column, lane) in lanes.iter().enumerate() {
            let Some(color) = *lane else { continue };
            let column = column as u16;
            segments.push(if ends(usize::from(column)) {
                Segment {
                    kind: SegmentKind::In,
                    from: column,
                    to: stored.column,
                    color,
                }
            } else {
                Segment {
                    kind: SegmentKind::Pass,
                    from: column,
                    to: column,
                    color,
                }
            });
        }
        // Every lane ending at the commit comes before every lane it leads
        // into, as `Lanes::place` records them: a column freed by one can be
        // taken by the other.
        for change in changes {
            let column = usize::from(change.column);
            if change.out {
                segments.push(Segment {
                    kind: SegmentKind::Out,
                    from: stored.column,
                    to: change.column,
                    color: change.color,
                });
                if lanes.len() <= column {
                    lanes.resize(column + 1, None);
                }
                lanes[column] = Some(change.color);
            } else {
                lanes[column] = None;
            }
        }
        while lanes.last() == Some(&None) {
            lanes.pop();
        }

        Row {
            column: stored.column,
            color: stored.color,
            segments,
        }
    }
}

/// The lanes open at one moment of the layout.
#[derive(Default)]
struct Lanes {
    lanes: Vec<Option<Lane>>,
    /// How many open lanes have each colour.
    used: [u32; COLORS as usize],
    /// The colour the next new lane looks at first, so new lanes take turns.
    turn: u8,
}

#[derive(Clone, Copy)]
struct Lane {
    /// The commit the lane leads down to.
    waits_for: ObjectId,
    color: u8,
}

impl Lanes {
    /// Places the commit `id` with these `parents`, records what it changes
    /// in `changes`, and returns its column and colour.
    fn place(
        &mut self,
        id: ObjectId,
        parents: &[ObjectId],
        changes: &mut Vec<Change>,
    ) -> (u16, u8) {
        let begin = changes.len();
        let mut column = None;
        for (at, lane) in self.lanes.iter().enumerate() {
            if lane.is_some_and(|lane| lane.waits_for == id) {
                column.get_or_insert(at);
                changes.push(Change {
                    out: false,
                    column: at as u16,
                    color: 0,
                });
            }
        }
        let (column, color) = match column {
            Some(column) => (column, self.lanes[column].expect("a waiting lane").color),
            // Nothing waits for it: the tip of a branch.
            None => (self.free(0), self.new_color()),
        };
        for change in &changes[begin..] {
            self.close(usize::from(change.column));
        }

        let mut named: Vec<ObjectId> = Vec::with_capacity(parents.len());
        for &parent in parents {
            // A parent named twice is one line, not two on top of each other.
            if named.contains(&parent) {
                continue;
            }
            named.push(parent);
            let (to, color) = if named.len() == 1 {
                (column, color)
            } else if let Some(waiting) = self
                .lanes
                .iter()
                .position(|lane| lane.is_some_and(|lane| lane.waits_for == parent))
            {
                (waiting, self.lanes[waiting].expect("a waiting lane").color)
            } else {
                (self.free(column + 1), self.new_color())
            };
            if self.lanes.get(to).is_none_or(Option::is_none) {
                self.open(to, parent, color);
            }
            changes.push(Change {
                out: true,
                column: to as u16,
                color,
            });
        }
        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        (column as u16, color)
    }

    /// The leftmost free column at or right of `from`.
    fn free(&self, from: usize) -> usize {
        (from..self.lanes.len())
            .find(|&at| self.lanes[at].is_none())
            .unwrap_or(self.lanes.len().max(from))
    }

    /// The colour fewest open lanes have; among those, the next in turn.
    fn new_color(&mut self) -> u8 {
        let color = (0..COLORS)
            .map(|step| (self.turn + step) % COLORS)
            .min_by_key(|&color| self.used[usize::from(color)])
            .expect("there are colours");
        self.turn = (color + 1) % COLORS;
        color
    }

    fn open(&mut self, at: usize, waits_for: ObjectId, color: u8) {
        if self.lanes.len() <= at {
            self.lanes.resize(at + 1, None);
        }
        self.lanes[at] = Some(Lane { waits_for, color });
        self.used[usize::from(color)] += 1;
    }

    fn close(&mut self, at: usize) {
        if let Some(lane) = self.lanes[at].take() {
            self.used[usize::from(lane.color)] -= 1;
        }
    }

    fn colors(&self) -> Vec<Option<u8>> {
        self.lanes
            .iter()
            .map(|lane| lane.map(|lane| lane.color))
            .collect()
    }

    fn width(&self) -> u16 {
        u16::try_from(self.lanes.len()).unwrap_or(u16::MAX)
    }
}

/// What a search of the graph looks for: words in a commit's subject or its
/// author's name or address, compared without case, or the start of its id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    words: String,
    /// The query as the start of an id, when it can be one: four hex digits
    /// or more, as `git` takes an abbreviated id.
    id: Option<String>,
}

impl Query {
    /// The query for `text`; `None` when there is nothing in it to look for.
    pub fn new(text: &str) -> Option<Self> {
        let words = text.trim().to_lowercase();
        if words.is_empty() {
            return None;
        }
        let id = (words.len() >= 4 && words.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then(|| words.clone());
        Some(Self { words, id })
    }

    fn finds(&self, id: &[u8], subject: &[u8], name: &[u8], email: &[u8]) -> bool {
        if let Some(prefix) = &self.id
            && id.starts_with(prefix.as_bytes())
        {
            return true;
        }
        [subject, name, email].iter().any(|text| self.holds(text))
    }

    /// Whether `text` holds the words, without case. Bytes are compared
    /// directly when both sides are ASCII, as nearly every subject is: a
    /// search reads every commit of the history, and lowercasing a copy of
    /// each field was most of what it cost.
    fn holds(&self, text: &[u8]) -> bool {
        let words = self.words.as_bytes();
        if self.words.is_ascii() && text.is_ascii() {
            return text.len() >= words.len()
                && text
                    .windows(words.len())
                    .any(|window| window.eq_ignore_ascii_case(words));
        }
        String::from_utf8_lossy(text)
            .to_lowercase()
            .contains(&self.words)
    }
}
