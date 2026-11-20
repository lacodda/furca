//! A file a merge left unresolved, as the index holds it: the common
//! ancestor's version, ours and theirs - git's stages 1, 2 and 3 - and how
//! their lines go together.
//!
//! The lines are put together here, from the three stages, rather than read
//! back from the conflict markers git wrote into the file. The markers are
//! written in whichever `merge.conflictStyle` the person chose, leave the
//! ancestor out in the default one, and are gone from a file someone has
//! started editing; the stages are the same in every case. The file is cut
//! into [`Block`]s as `diff3` cuts it: where neither side changed anything,
//! where one did, where both made the same change, and where they disagree -
//! a conflict, the one kind a person has to choose for.
//!
//! What a merge left is resolved through git ([`Git::resolve`]), which writes
//! the index and the file through the repository's own filters (ADR 0023).
//!
//! [`Git::resolve`]: crate::Git::resolve

use gix::bstr::{BStr, ByteSlice};
use gix::diff::blob::{Algorithm, Diff, InternedInput};
use gix::index::entry::Stage;
use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::diff::{Content, FileChange, FileMode, FileStatus, Hunk, LineRange, LineSplit, Side};
use crate::error::{Error, Source, wrap};
use crate::status::{ConflictKind, Conflicted};

/// One file a merge, a cherry-pick, a revert, a rebase or a stash left
/// unresolved: what each side holds, and how their lines go together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Conflict {
    pub path: String,
    pub kind: ConflictKind,
    /// The common ancestor's file; `None` when it had none - both sides
    /// added the path.
    pub base: Option<Side>,
    /// Our file: the branch `HEAD` is on, or - in a rebase - the commits it
    /// is replaying onto. `None` when our side deleted it.
    pub ours: Option<Side>,
    /// Their file: what is being merged, picked, reverted or replayed.
    /// `None` when their side deleted it.
    pub theirs: Option<Side>,
    /// The three texts whole, when every side there is is text: a side with
    /// no file is empty. A `\r` before a line's end is kept; bytes that are
    /// not UTF-8 read as U+FFFD, which keeps every line where it was.
    pub texts: Option<Texts>,
    /// The lines in blocks, in order, covering every line of all three
    /// texts once. Empty unless both sides changed the file and every side is
    /// text: a file one side deleted, or a binary, is taken whole, from one
    /// side or the other.
    pub blocks: Vec<Block>,
    /// Where the file in the working tree still holds the conflict markers
    /// git wrote into it: the line, from 1, of the first. `None` when it
    /// holds none - resolved by hand or by another tool already - or is gone.
    pub marked: Option<usize>,
}

/// The three texts of a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Texts {
    pub base: String,
    pub ours: String,
    pub theirs: String,
}

/// A run of lines and what became of it on each side: its lines in the
/// ancestor, in ours and in theirs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Block {
    pub kind: BlockKind,
    pub base: LineRange,
    pub ours: LineRange,
    pub theirs: LineRange,
}

/// What became of a block's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum BlockKind {
    /// The same on all three: it goes in as it is.
    Unchanged,
    /// Only our side changed it: ours goes in.
    Ours,
    /// Only their side changed it: theirs goes in.
    Theirs,
    /// Both sides made the same change: it goes in once.
    Same,
    /// Both sides changed it, differently: someone has to choose.
    Conflict,
}

/// What a conflicting block takes, or every one of them at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Pick {
    Ours,
    Theirs,
    /// Ours, then theirs.
    Both,
    /// The ancestor's lines: neither side's change.
    Base,
}

/// How a conflicted file is resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "take", rename_all = "kebab-case")]
pub enum Resolution {
    /// The file as the working tree has it now, edited by hand or by another
    /// tool; deleted, if the working tree no longer has it. Refused while it
    /// still holds conflict markers, unless `force`.
    AsIs { force: bool },
    /// Our file whole, as `git checkout --ours` takes it - and none at all
    /// when our side deleted it. Their changes to it are left out, the ones
    /// that did not conflict too.
    Ours,
    /// Their file whole, on the same terms.
    Theirs,
    /// A file with `text`, in the repository's form - lines ending `\n`,
    /// given the ancestor's endings back when they were `\r\n` - written
    /// to the working tree through its filters: what a resolver made.
    Text { text: String },
    /// No file at all.
    Delete,
}

impl Conflict {
    /// The file with every conflicting block taken from `pick`, and every
    /// other block as it goes in on its own; `None` when it is not cut into
    /// blocks. Lines end `\n`, a `\r` before it kept as the sides have it.
    pub fn merged(&self, pick: Pick) -> Option<String> {
        let texts = self.texts.as_ref()?;
        if self.blocks.is_empty() {
            return None;
        }
        let base = lines(&texts.base);
        let ours = lines(&texts.ours);
        let theirs = lines(&texts.theirs);
        let mut out = String::with_capacity(texts.ours.len().max(texts.theirs.len()));
        for block in &self.blocks {
            match (block.kind, pick) {
                (BlockKind::Theirs, _) | (BlockKind::Conflict, Pick::Theirs) => {
                    push_lines(&mut out, &theirs, block.theirs);
                }
                (BlockKind::Conflict, Pick::Base) => push_lines(&mut out, &base, block.base),
                (BlockKind::Conflict, Pick::Both) => {
                    push_lines(&mut out, &ours, block.ours);
                    push_lines(&mut out, &theirs, block.theirs);
                }
                _ => push_lines(&mut out, &ours, block.ours),
            }
        }
        Some(out)
    }

    /// How many blocks conflict.
    pub fn conflicts(&self) -> usize {
        self.blocks
            .iter()
            .filter(|block| block.kind == BlockKind::Conflict)
            .count()
    }
}

impl Repository {
    /// Every path the index holds unresolved, by path, with how the two
    /// sides disagree: read from the index's stages, without looking at the
    /// working tree.
    pub fn conflicts(&self) -> Result<Vec<Conflicted>, Error> {
        let index = self.inner.index_or_empty().map_err(wrap(Error::Index))?;
        let mut out = Vec::new();
        let mut current: Option<(&BStr, [bool; 3])> = None;
        for entry in index.entries() {
            let stage = entry.stage();
            if stage == Stage::Unconflicted {
                continue;
            }
            let path = entry.path(&index);
            match current.as_mut() {
                Some((at, stages)) if *at == path => stages[stage as usize - 1] = true,
                _ => {
                    out.extend(current.and_then(conflicted));
                    let mut stages = [false; 3];
                    stages[stage as usize - 1] = true;
                    current = Some((path, stages));
                }
            }
        }
        out.extend(current.and_then(conflicted));
        Ok(out)
    }

    /// The conflict at `path`: its three sides, and its lines in blocks.
    pub fn conflict(&self, path: &str) -> Result<Conflict, Error> {
        let failed = |source: Source| Error::Diff {
            path: path.to_owned(),
            source,
        };
        let index = self.inner.index_or_empty().map_err(wrap(Error::Index))?;
        let stage = |stage: Stage| {
            index
                .entry_by_path_and_stage(path.as_bytes().as_bstr(), stage)
                .map(|entry| (entry.id, entry.mode))
        };
        let (base, ours, theirs) = (stage(Stage::Base), stage(Stage::Ours), stage(Stage::Theirs));
        let Some(kind) = kind_of_stages([base.is_some(), ours.is_some(), theirs.is_some()]) else {
            return Err(Error::Operation(format!("{path} is not in conflict")));
        };
        let side = |stage: Option<(gix::ObjectId, gix::index::entry::Mode)>| {
            stage.map(|(id, mode)| Side {
                id: Some(id.to_string()),
                mode: crate::status::file_mode(mode),
                content: Content::Unread,
                size: None,
            })
        };
        let (base, ours, theirs) = (side(base), side(ours), side(theirs));

        // Each side is read as a commit's change from the ancestor reads it:
        // through the repository's attributes, so a binary, an LFS pointer
        // or a file too large to compare is said to be one.
        let mut cache = self
            .inner
            .diff_resource_cache_for_tree_diff()
            .map_err(|e| failed(e.into()))?;
        let change = |after: &Option<Side>| FileChange {
            path: path.to_owned(),
            status: match (&base, after) {
                (None, _) => FileStatus::Added,
                (_, None) => FileStatus::Deleted,
                _ => FileStatus::Modified,
            },
            from: None,
            similarity: None,
            before: base.clone(),
            after: after.clone(),
            lines: None,
        };
        let to_ours = self.compare_keeping(&mut cache, &change(&ours), true)?;
        let to_theirs = self.compare_keeping(&mut cache, &change(&theirs), true)?;
        let described = |side: &Option<Side>, read: &Option<Side>| {
            side.as_ref().and(read.clone()).or_else(|| side.clone())
        };
        let ours = described(&ours, &to_ours.after);
        let theirs = described(&theirs, &to_theirs.after);
        let base = described(&base, &to_ours.before);

        let text = |side: &Option<Side>| {
            side.as_ref().is_none_or(|side| {
                matches!(side.content, Content::Text { .. }) && side.mode != FileMode::Submodule
            })
        };
        let all_text = text(&base) && text(&ours) && text(&theirs);
        let (texts, blocks) = match (&to_ours.bytes, &to_theirs.bytes) {
            (Some((base_bytes, ours_bytes)), Some((_, theirs_bytes))) if all_text => {
                let texts = Texts {
                    base: String::from_utf8_lossy(base_bytes).into_owned(),
                    ours: String::from_utf8_lossy(ours_bytes).into_owned(),
                    theirs: String::from_utf8_lossy(theirs_bytes).into_owned(),
                };
                let blocks = if matches!(kind, ConflictKind::BothModified | ConflictKind::BothAdded)
                {
                    blocks(
                        &split(base_bytes),
                        &split(ours_bytes),
                        &split(theirs_bytes),
                        &to_ours.hunks,
                        &to_theirs.hunks,
                    )
                } else {
                    Vec::new()
                };
                (Some(texts), blocks)
            }
            _ => (None, Vec::new()),
        };
        let marked = self
            .workdir()
            .and_then(|dir| std::fs::read(dir.join(path)).ok())
            .and_then(|bytes| markers(&bytes));
        Ok(Conflict {
            path: path.to_owned(),
            kind,
            base,
            ours,
            theirs,
            texts,
            blocks,
            marked,
        })
    }

    /// The id and mode the index holds at `path` in `stage`.
    pub(crate) fn staged_at(
        &self,
        path: &str,
        stage: Stage,
    ) -> Result<Option<(String, u32)>, Error> {
        let index = self.inner.index_or_empty().map_err(wrap(Error::Index))?;
        Ok(index
            .entry_by_path_and_stage(path.as_bytes().as_bstr(), stage)
            .map(|entry| (entry.id.to_string(), entry.mode.bits())))
    }
}

/// A path the index holds in `stages`, as a conflict.
fn conflicted((path, stages): (&BStr, [bool; 3])) -> Option<Conflicted> {
    kind_of_stages(stages).map(|kind| Conflicted {
        path: path.to_str_lossy().into_owned(),
        kind,
    })
}

/// How two sides disagree, from which of the stages - base, ours, theirs -
/// the index holds; `None` for none of them.
fn kind_of_stages([base, ours, theirs]: [bool; 3]) -> Option<ConflictKind> {
    Some(match (base, ours, theirs) {
        (true, true, true) => ConflictKind::BothModified,
        (false, true, true) => ConflictKind::BothAdded,
        (true, false, false) => ConflictKind::BothDeleted,
        (false, true, false) => ConflictKind::AddedByUs,
        (false, false, true) => ConflictKind::AddedByThem,
        (true, false, true) => ConflictKind::DeletedByUs,
        (true, true, false) => ConflictKind::DeletedByThem,
        (false, false, false) => return None,
    })
}

/// The lines `range` names of `lines` onto `out`. A last line without its
/// newline, with more after it, is given one: two lines must not run into
/// one.
fn push_lines(out: &mut String, lines: &[&str], range: LineRange) {
    let start = (range.start as usize).min(lines.len());
    let end = (start + range.count as usize).min(lines.len());
    for line in &lines[start..end] {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(line);
    }
}

/// The lines of `text`, each with its `\n`.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// The lines of `bytes` as the diff splits them: without their `\n`.
fn split(bytes: &[u8]) -> Vec<&[u8]> {
    LineSplit(bytes).collect()
}

/// A change one side made: lines of the ancestor, and the lines that took
/// their place.
#[derive(Debug, Clone, Copy)]
struct Change {
    ours: bool,
    base: (u32, u32),
    side: (u32, u32),
}

impl Change {
    fn of(hunk: &Hunk, ours: bool) -> Change {
        Change {
            ours,
            base: (hunk.before.start, hunk.before.start + hunk.before.count),
            side: (hunk.after.start, hunk.after.start + hunk.after.count),
        }
    }

    /// How many lines the change adds, less those it takes away.
    fn grows(&self) -> i64 {
        i64::from(self.side.1 - self.side.0) - i64::from(self.base.1 - self.base.0)
    }
}

/// The three texts cut into blocks, as `diff3` cuts them, from what each side
/// changed of the ancestor.
///
/// Changes from the two sides that overlap, or only touch, are one region:
/// as in git, two changes with no line between them are a conflict, since
/// either one may have been written knowing the line the other replaced.
fn blocks(
    base: &[&[u8]],
    ours: &[&[u8]],
    theirs: &[&[u8]],
    to_ours: &[Hunk],
    to_theirs: &[Hunk],
) -> Vec<Block> {
    let mut changes: Vec<Change> = to_ours
        .iter()
        .map(|hunk| Change::of(hunk, true))
        .chain(to_theirs.iter().map(|hunk| Change::of(hunk, false)))
        .collect();
    changes.sort_by_key(|change| (change.base.0, change.base.1, !change.ours));

    let mut out = Vec::new();
    // Where the last region ended in the ancestor, and how far each side's
    // lines have moved from the ancestor's by then.
    let mut at = 0u32;
    let (mut ours_by, mut theirs_by) = (0i64, 0i64);
    let mut next = 0;
    while next < changes.len() {
        let start = changes[next].base.0;
        let mut end = changes[next].base.1;
        let mut last = next + 1;
        while last < changes.len() && changes[last].base.0 <= end {
            end = end.max(changes[last].base.1);
            last += 1;
        }
        let region = &changes[next..last];
        if start > at {
            out.push(Block {
                kind: BlockKind::Unchanged,
                base: span(at, start),
                ours: span(moved(at, ours_by), moved(start, ours_by)),
                theirs: span(moved(at, theirs_by), moved(start, theirs_by)),
            });
        }
        let ours_grows: i64 = region.iter().filter(|c| c.ours).map(Change::grows).sum();
        let theirs_grows: i64 = region.iter().filter(|c| !c.ours).map(Change::grows).sum();
        let in_ours = span(moved(start, ours_by), moved(end, ours_by + ours_grows));
        let in_theirs = span(
            moved(start, theirs_by),
            moved(end, theirs_by + theirs_grows),
        );
        let block = Block {
            kind: BlockKind::Conflict,
            base: span(start, end),
            ours: in_ours,
            theirs: in_theirs,
        };
        let by_ours = region.iter().any(|c| c.ours);
        let by_theirs = region.iter().any(|c| !c.ours);
        if !by_theirs {
            out.push(Block {
                kind: BlockKind::Ours,
                ..block
            });
        } else if !by_ours {
            out.push(Block {
                kind: BlockKind::Theirs,
                ..block
            });
        } else if slice(ours, in_ours) == slice(theirs, in_theirs) {
            out.push(Block {
                kind: BlockKind::Same,
                ..block
            });
        } else {
            refine(block, ours, theirs, &mut out);
        }
        ours_by += ours_grows;
        theirs_by += theirs_grows;
        at = end;
        next = last;
    }
    let base_len = u32::try_from(base.len()).unwrap_or(u32::MAX);
    if at < base_len || out.is_empty() {
        out.push(Block {
            kind: BlockKind::Unchanged,
            base: span(at, base_len),
            ours: span(moved(at, ours_by), moved(base_len, ours_by)),
            theirs: span(moved(at, theirs_by), moved(base_len, theirs_by)),
        });
    }
    out.retain(|block| block.base.count + block.ours.count + block.theirs.count > 0);
    out
}

/// A conflict made as small as the two sides allow, the lines both of them
/// wrote taken out of it as blocks of their own.
///
/// Where the ancestor had lines there, only what the two sides begin and end
/// with alike is taken out: the ancestor's lines stay with the conflict whole,
/// since no one can say which of them a line both sides wrote replaced. Where
/// it had none - both sides only added, a file both added - the two are
/// compared line by line, and every run they share is a block of its own.
fn refine(block: Block, ours: &[&[u8]], theirs: &[&[u8]], out: &mut Vec<Block>) {
    let ours_lines = slice(ours, block.ours);
    let theirs_lines = slice(theirs, block.theirs);
    let at_base = block.base.start;
    let same = |ours: LineRange, theirs: LineRange| Block {
        kind: BlockKind::Same,
        base: LineRange {
            start: at_base,
            count: 0,
        },
        ours,
        theirs,
    };
    if block.base.count == 0 {
        let input = InternedInput::new(LineSplitOf(ours_lines), LineSplitOf(theirs_lines));
        let mut diff = Diff::compute(Algorithm::Histogram, &input);
        diff.postprocess_lines(&input);
        let (mut o, mut t) = (0u32, 0u32);
        let shifted = |range: std::ops::Range<u32>, by: u32| LineRange {
            start: range.start + by,
            count: range.end - range.start,
        };
        for hunk in diff.hunks() {
            if hunk.before.start > o {
                out.push(same(
                    shifted(o..hunk.before.start, block.ours.start),
                    shifted(t..hunk.after.start, block.theirs.start),
                ));
            }
            out.push(Block {
                kind: BlockKind::Conflict,
                base: LineRange {
                    start: at_base,
                    count: 0,
                },
                ours: shifted(hunk.before.clone(), block.ours.start),
                theirs: shifted(hunk.after.clone(), block.theirs.start),
            });
            o = hunk.before.end;
            t = hunk.after.end;
        }
        if o < block.ours.count {
            out.push(same(
                shifted(o..block.ours.count, block.ours.start),
                shifted(t..block.theirs.count, block.theirs.start),
            ));
        }
        return;
    }
    let most = ours_lines.len().min(theirs_lines.len());
    let head = (0..most)
        .take_while(|&i| ours_lines[i] == theirs_lines[i])
        .count();
    let tail = (0..most - head)
        .take_while(|&i| {
            ours_lines[ours_lines.len() - 1 - i] == theirs_lines[theirs_lines.len() - 1 - i]
        })
        .count();
    let (head, tail) = (head as u32, tail as u32);
    if head > 0 {
        out.push(same(
            LineRange {
                start: block.ours.start,
                count: head,
            },
            LineRange {
                start: block.theirs.start,
                count: head,
            },
        ));
    }
    out.push(Block {
        kind: BlockKind::Conflict,
        base: block.base,
        ours: LineRange {
            start: block.ours.start + head,
            count: block.ours.count - head - tail,
        },
        theirs: LineRange {
            start: block.theirs.start + head,
            count: block.theirs.count - head - tail,
        },
    });
    if tail > 0 {
        let at_end = |range: LineRange| LineRange {
            start: range.start + range.count - tail,
            count: tail,
        };
        out.push(Block {
            base: LineRange {
                start: block.base.start + block.base.count,
                count: 0,
            },
            ..same(at_end(block.ours), at_end(block.theirs))
        });
    }
}

/// Lines already split, as a diff takes them.
#[derive(Clone, Copy)]
struct LineSplitOf<'a>(&'a [&'a [u8]]);

impl<'a> gix::diff::blob::TokenSource for LineSplitOf<'a> {
    type Token = &'a [u8];
    type Tokenizer = std::iter::Copied<std::slice::Iter<'a, &'a [u8]>>;

    fn tokenize(&self) -> Self::Tokenizer {
        self.0.iter().copied()
    }

    fn estimate_tokens(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap_or(u32::MAX)
    }
}

fn span(start: u32, end: u32) -> LineRange {
    LineRange {
        start,
        count: end.saturating_sub(start),
    }
}

/// A line of the ancestor as the line it is on a side moved `by`.
fn moved(line: u32, by: i64) -> u32 {
    u32::try_from(i64::from(line) + by).unwrap_or(0)
}

fn slice<'a>(lines: &'a [&'a [u8]], range: LineRange) -> &'a [&'a [u8]] {
    let start = (range.start as usize).min(lines.len());
    let end = (start + range.count as usize).min(lines.len());
    &lines[start..end]
}

/// Where a file a merge left still holds the markers git writes into it:
/// the line, from 1, of the first `<<<<<<<` that a `>>>>>>>` closes further
/// down. A line of `=======` alone is not one - it underlines a Markdown
/// heading as often as it parts a conflict.
pub(crate) fn markers(text: &[u8]) -> Option<usize> {
    let marker = |line: &[u8], sign: u8| {
        line.len() >= 7
            && line[..7].iter().all(|&byte| byte == sign)
            && line
                .get(7)
                .is_none_or(|&byte| byte == b' ' || byte == b'\r' || byte == b'\t')
    };
    let mut opened = None;
    for (at, line) in LineSplit(text).enumerate() {
        if opened.is_none() && marker(line, b'<') {
            opened = Some(at + 1);
        } else if opened.is_some() && marker(line, b'>') {
            return opened;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{BlockKind, Conflict, Pick, Texts, blocks, markers, split};
    use crate::diff::{Hunk, LineRange};
    use crate::status::ConflictKind;

    fn range(start: u32, count: u32) -> LineRange {
        LineRange { start, count }
    }

    fn hunk(before: (u32, u32), after: (u32, u32)) -> Hunk {
        Hunk {
            before: range(before.0, before.1),
            after: range(after.0, after.1),
        }
    }

    /// The hunks between two texts as the core finds them.
    fn hunks(a: &str, b: &str) -> Vec<Hunk> {
        use gix::diff::blob::{Algorithm, Diff, InternedInput};
        let input = InternedInput::new(
            super::LineSplit(a.as_bytes()),
            super::LineSplit(b.as_bytes()),
        );
        let mut diff = Diff::compute(Algorithm::Myers, &input);
        diff.postprocess_lines(&input);
        diff.hunks()
            .map(|h| {
                hunk(
                    (h.before.start, h.before.end - h.before.start),
                    (h.after.start, h.after.end - h.after.start),
                )
            })
            .collect()
    }

    fn conflict(base: &str, ours: &str, theirs: &str) -> Conflict {
        let found = blocks(
            &split(base.as_bytes()),
            &split(ours.as_bytes()),
            &split(theirs.as_bytes()),
            &hunks(base, ours),
            &hunks(base, theirs),
        );
        Conflict {
            path: "f".to_owned(),
            kind: ConflictKind::BothModified,
            base: None,
            ours: None,
            theirs: None,
            texts: Some(Texts {
                base: base.to_owned(),
                ours: ours.to_owned(),
                theirs: theirs.to_owned(),
            }),
            blocks: found,
            marked: None,
        }
    }

    fn kinds(conflict: &Conflict) -> Vec<BlockKind> {
        conflict.blocks.iter().map(|block| block.kind).collect()
    }

    #[test]
    fn changes_apart_go_in_from_both_sides_and_only_a_meeting_is_a_conflict() {
        let base = "a\nb\nc\nd\ne\nf\ng\n";
        let ours = "A\nb\nc\nd\ne\nF\ng\n";
        let theirs = "a\nb\nc\nD\ne\nf2\ng\n";
        let c = conflict(base, ours, theirs);
        assert_eq!(
            kinds(&c),
            [
                BlockKind::Ours,
                BlockKind::Unchanged,
                BlockKind::Theirs,
                BlockKind::Unchanged,
                BlockKind::Conflict,
                BlockKind::Unchanged
            ]
        );
        assert_eq!(c.conflicts(), 1);
        assert_eq!(
            c.merged(Pick::Ours).as_deref(),
            Some("A\nb\nc\nD\ne\nF\ng\n")
        );
        assert_eq!(
            c.merged(Pick::Theirs).as_deref(),
            Some("A\nb\nc\nD\ne\nf2\ng\n")
        );
        assert_eq!(
            c.merged(Pick::Both).as_deref(),
            Some("A\nb\nc\nD\ne\nF\nf2\ng\n")
        );
        assert_eq!(
            c.merged(Pick::Base).as_deref(),
            Some("A\nb\nc\nD\ne\nf\ng\n")
        );
    }

    #[test]
    fn two_changes_that_touch_are_one_conflict_as_in_git() {
        // Ours rewrites b, theirs rewrites c, the line after it: no line
        // between them, so they meet.
        let c = conflict("a\nb\nc\nd\n", "a\nB\nc\nd\n", "a\nb\nC\nd\n");
        assert_eq!(
            kinds(&c),
            [
                BlockKind::Unchanged,
                BlockKind::Conflict,
                BlockKind::Unchanged
            ]
        );
        let conflict = c.blocks[1];
        assert_eq!(conflict.base, range(1, 2));
        assert_eq!(conflict.ours, range(1, 2));
        assert_eq!(conflict.theirs, range(1, 2));
    }

    #[test]
    fn the_same_change_on_both_sides_goes_in_once() {
        let c = conflict("a\nb\nc\n", "a\nX\nc\n", "a\nX\nc\n");
        assert_eq!(
            kinds(&c),
            [BlockKind::Unchanged, BlockKind::Same, BlockKind::Unchanged]
        );
        assert_eq!(c.conflicts(), 0);
        assert_eq!(c.merged(Pick::Theirs).as_deref(), Some("a\nX\nc\n"));
    }

    #[test]
    fn what_both_sides_begin_and_end_with_alike_is_taken_out_of_a_conflict() {
        let c = conflict("a\nold\nz\n", "a\nx\nmine\ny\nz\n", "a\nx\ntheirs\ny\nz\n");
        assert_eq!(
            kinds(&c),
            [
                BlockKind::Unchanged,
                BlockKind::Same,
                BlockKind::Conflict,
                BlockKind::Same,
                BlockKind::Unchanged
            ]
        );
        let conflict = c.blocks[2];
        assert_eq!(
            conflict.base,
            range(1, 1),
            "the ancestor's line stays with the conflict"
        );
        assert_eq!(conflict.ours, range(2, 1));
        assert_eq!(conflict.theirs, range(2, 1));
        assert_eq!(c.merged(Pick::Base).as_deref(), Some("a\nx\nold\ny\nz\n"));
    }

    #[test]
    fn a_file_both_added_conflicts_only_where_the_two_differ() {
        let c = conflict(
            "",
            "use a;\nfn one() {}\nfn end() {}\n",
            "use a;\nfn two() {}\nfn end() {}\n",
        );
        assert_eq!(
            kinds(&c),
            [BlockKind::Same, BlockKind::Conflict, BlockKind::Same]
        );
        assert_eq!(
            c.merged(Pick::Both).as_deref(),
            Some("use a;\nfn one() {}\nfn two() {}\nfn end() {}\n")
        );
    }

    #[test]
    fn every_line_of_every_text_is_in_exactly_one_block() {
        let cases = [
            ("a\nb\nc\n", "a\nb\nc\nd\n", "z\na\nb\nc\n"),
            ("a\nb\n", "", "a\nb\nc\n"),
            ("1\n2\n3\n4\n5\n", "1\n3\n5\n", "1\n2\n4\n5\nsix\n"),
            ("x", "y", "x\nz"),
        ];
        for (base, ours, theirs) in cases {
            let c = conflict(base, ours, theirs);
            for (side, text) in [("base", base), ("ours", ours), ("theirs", theirs)] {
                let mut next = 0;
                for block in &c.blocks {
                    let range = match side {
                        "base" => block.base,
                        "ours" => block.ours,
                        _ => block.theirs,
                    };
                    assert_eq!(
                        range.start, next,
                        "{side} of {base:?}/{ours:?}/{theirs:?}: {:?}",
                        c.blocks
                    );
                    next += range.count;
                }
                assert_eq!(
                    next as usize,
                    split(text.as_bytes()).len(),
                    "{side} of {base:?}/{ours:?}/{theirs:?}"
                );
            }
        }
    }

    #[test]
    fn a_last_line_without_its_newline_does_not_run_into_the_next() {
        let c = conflict("a\n", "a\nmine", "a\ntheirs");
        assert_eq!(c.merged(Pick::Both).as_deref(), Some("a\nmine\ntheirs"));
    }

    #[test]
    fn markers_are_found_where_a_conflict_is_still_written_out() {
        let marked = b"a\n<<<<<<< HEAD\nmine\n=======\ntheirs\n>>>>>>> feature\nb\n";
        assert_eq!(markers(marked), Some(2));
        assert_eq!(markers(b"<<<<<<<\r\nx\r\n>>>>>>>\r\n"), Some(1));
        assert_eq!(
            markers(b"Title\n=======\n\ntext\n"),
            None,
            "a Markdown heading"
        );
        assert_eq!(markers(b"<<<<<<<< eight\n>>>>>>>> eight\n"), None);
        assert_eq!(markers(b"<<<<<<< opened and never closed\n"), None);
    }
}
