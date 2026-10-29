//! What a commit changed: which files, how, and the lines.
//!
//! A commit is compared with its first parent - a merge with the line it was
//! merged into, as Fork shows it - and a root commit with nothing. Renames
//! are found as git finds them, by content, under the repository's own
//! `diff.renames`.
//!
//! Every side of a change says what it holds before anything is drawn: text
//! and how its lines end, a binary, a git LFS pointer, a submodule's commit,
//! and its size. A binary that slipped into a public repository is the most
//! expensive mistake a commit can make, so it is a fact of the change, not of
//! a diff someone may never open.
//!
//! The lines are compared as git compares them: the algorithm
//! `diff.algorithm` names, with git's slider heuristics, and a line that only
//! changed its ending is a changed line.

use gix::bstr::{BStr, ByteSlice};
use gix::diff::blob::platform::prepare_diff::Operation;
use gix::diff::blob::platform::resource::Data;
use gix::diff::blob::{Diff, InternedInput, ResourceKind, TokenSource};
use gix::object::tree::EntryKind;
use gix::object::tree::diff::ChangeDetached;
use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::error::{Error, Source};
use crate::log::Commit;

/// What one commit changed against its first parent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Changes {
    /// The commit, as a full hex id.
    pub commit: String,
    /// The parent it is compared with - the first, for a merge; `None` for a
    /// root commit, which is compared with nothing.
    pub parent: Option<String>,
    /// The files that differ, by path.
    pub files: Vec<FileChange>,
}

/// A commit as a reader opens it: who made it and when, the whole message,
/// and what it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Details {
    pub commit: Commit,
    /// The whole message: subject, body and trailers.
    pub message: String,
    pub changes: Changes,
}

/// One file a commit added, deleted, changed, renamed or copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileChange {
    /// Where the file is after the commit - for a deletion, where it was -
    /// from the root of the repository with `/` separators.
    pub path: String,
    pub status: FileStatus,
    /// Where a renamed or copied file came from.
    pub from: Option<String>,
    /// How much of a renamed or copied file's content it kept, in percent.
    pub similarity: Option<u8>,
    /// The file before the commit; `None` when the commit added it.
    pub before: Option<Side>,
    /// The file after the commit; `None` when the commit deleted it.
    pub after: Option<Side>,
    /// Lines added and removed; `None` when a side is not text - a binary,
    /// a git LFS pointer, a submodule - and there are no lines to count.
    pub lines: Option<LineCount>,
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum FileStatus {
    Added,
    Deleted,
    /// The same path with other content, another mode, or both.
    Modified,
    /// Moved to another path, perhaps with changes on the way.
    Renamed,
    /// A new path made from an existing file, which stays where it was.
    Copied,
}

/// One side of a change: the object, and what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Side {
    /// The blob's id, or the commit a submodule is at.
    pub id: String,
    pub mode: FileMode,
    pub content: Content,
    /// The size of the file as it is checked out, in bytes: for a git LFS
    /// pointer, the size of the file it stands for. `None` for a submodule.
    pub size: Option<u64>,
}

/// What kind of entry a side is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum FileMode {
    File,
    Executable,
    /// A symbolic link; its content is the path it points to.
    Link,
    Submodule,
}

/// What a side holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Content {
    /// Lines a diff can compare.
    Text { endings: Endings },
    /// Bytes git does not compare by lines: a NUL among the first 8000
    /// bytes, the `binary` or `-diff` attribute, or more than
    /// `core.bigFileThreshold`.
    Binary,
    /// The pointer git LFS commits in place of a file, which lives in LFS
    /// storage under `oid`, a SHA-256.
    Lfs { oid: String },
    /// A submodule's commit rather than a file.
    Submodule,
}

/// How the lines of a text end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Endings {
    Lf,
    Crlf,
    /// Some lines end one way and some the other.
    Mixed,
    /// No line ends: the file is empty, or one line without a newline.
    NoEnding,
}

/// Lines a change added and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LineCount {
    pub added: u32,
    pub removed: u32,
}

/// One file's change with its lines: both texts whole, and where they
/// differ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct FileDiff {
    pub file: FileChange,
    /// The text before the commit, whole: what a side-by-side view shows.
    /// `None` when there was no file or it is not text. Bytes that are not
    /// UTF-8 read as U+FFFD, which keeps every line where it was.
    pub before: Option<String>,
    /// The text after the commit, on the same terms.
    pub after: Option<String>,
    /// Where the lines differ, in order, without context.
    pub hunks: Vec<Hunk>,
}

/// Lines of the text before that became lines of the text after. One of the
/// two is empty for a pure addition or removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Hunk {
    pub before: LineRange,
    pub after: LineRange,
}

/// A run of lines: the first, counted from 0, and how many. An empty run
/// sits before the line `start` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct LineRange {
    pub start: u32,
    pub count: u32,
}

/// One file's change as a patch reads: hunks with the lines around them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Patch {
    pub file: FileChange,
    pub hunks: Vec<PatchHunk>,
}

/// A hunk of a patch: the lines it spans on each side, context included,
/// and every line in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PatchHunk {
    pub before: LineRange,
    pub after: LineRange,
    pub lines: Vec<PatchLine>,
}

/// A line of a patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct PatchLine {
    pub kind: LineKind,
    /// The line without its newline; a `\r` before it is kept.
    pub text: String,
}

/// Whether a line of a patch is kept, removed or added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum LineKind {
    Context,
    Removed,
    Added,
}

/// The most bytes a git LFS pointer may have; anything longer is a file.
const LFS_POINTER_MOST: usize = 1024;

impl Repository {
    /// What the commit `revision` names changed against its first parent:
    /// every file, how it changed, what each side holds and how many lines
    /// moved. A root commit is compared with nothing, so all its files are
    /// added.
    pub fn changes(&self, revision: &str) -> Result<Changes, Error> {
        let id = self.resolve(revision)?;
        let failed = |source: Source| Error::Changes {
            commit: id.to_string(),
            source,
        };
        let commit = self.inner.find_commit(id).map_err(|e| failed(e.into()))?;
        let parent = commit.parent_ids().next().map(|parent| parent.detach());
        let after = commit.tree().map_err(|e| failed(e.into()))?;
        let before = match parent {
            Some(parent) => Some(
                self.inner
                    .find_commit(parent)
                    .and_then(|parent| parent.tree())
                    .map_err(|e| failed(e.into()))?,
            ),
            None => None,
        };
        // One cache for the tree diff's rename checks and the line counts:
        // making one reads the index and the attributes, which is most of
        // what a small commit costs.
        let mut cache = self
            .inner
            .diff_resource_cache_for_tree_diff()
            .map_err(|e| failed(e.into()))?;
        let changes = self
            .tree_changes(before.as_ref(), &after, &mut cache)
            .map_err(failed)?;
        let mut files = Vec::with_capacity(changes.len());
        for change in changes {
            let Some(mut file) = file_change(change) else {
                continue;
            };
            let compared = self.compare(&mut cache, &file)?;
            file.before = compared.before;
            file.after = compared.after;
            file.lines = compared.lines;
            files.push(file);
        }
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Changes {
            commit: id.to_string(),
            parent: parent.map(|parent| parent.to_string()),
            files,
        })
    }

    /// The changes between two trees, renames found under the repository's
    /// `diff.renames` - on unless it says otherwise, as in git - with
    /// `cache` reading the blobs a rename is judged by.
    pub(crate) fn tree_changes(
        &self,
        before: Option<&gix::Tree<'_>>,
        after: &gix::Tree<'_>,
        cache: &mut gix::diff::blob::Platform,
    ) -> Result<Vec<ChangeDetached>, Source> {
        let config = self.inner.config_snapshot();
        // A diff.renames that cannot be read is taken as unset rather than
        // failing the commit: it is a fact about the configuration.
        let (rewrites, configured) = gix::diff::new_rewrites(config.plumbing(), true)?;
        let rewrites = if configured {
            rewrites
        } else {
            Some(Default::default())
        };
        let options = gix::diff::Options::default().with_rewrites(rewrites);
        let empty = self.inner.empty_tree();
        let before = before.unwrap_or(&empty);
        let mut changes = Vec::new();
        gix::diff::tree_with_rewrites(
            gix::objs::TreeRefIter::from_bytes(&before.data, before.id.kind()),
            gix::objs::TreeRefIter::from_bytes(&after.data, after.id.kind()),
            cache,
            &mut Default::default(),
            &self.inner.objects,
            |change| {
                changes.push(change.into_owned());
                Ok(std::ops::ControlFlow::Continue(()))
            },
            options.into(),
        )?;
        Ok(changes)
    }

    /// A commit as a reader opens it: [`Repository::commit`], the whole
    /// message and [`Repository::changes`] in one read.
    pub fn details(&self, revision: &str) -> Result<Details, Error> {
        let commit = self.commit(revision)?;
        let message = self.message(&commit.id)?;
        let changes = self.changes(&commit.id)?;
        Ok(Details {
            commit,
            message,
            changes,
        })
    }

    /// One file's change with its lines: the two texts whole and the hunks
    /// between them, for a change [`Repository::changes`] listed. A side
    /// that is not text has no text and no hunks; its [`Side`] says what it
    /// is.
    pub fn diff(&self, file: &FileChange) -> Result<FileDiff, Error> {
        let failed = |source: Source| Error::Diff {
            path: file.path.clone(),
            source,
        };
        let mut cache = self
            .inner
            .diff_resource_cache_for_tree_diff()
            .map_err(|e| failed(e.into()))?;
        let compared = self.compare(&mut cache, file)?;
        let mut file = file.clone();
        file.before = compared.before;
        file.after = compared.after;
        file.lines = compared.lines;
        Ok(FileDiff {
            file,
            before: compared.texts.as_ref().and_then(|t| t.0.clone()),
            after: compared.texts.and_then(|t| t.1),
            hunks: compared.hunks,
        })
    }

    /// Reads both sides of `file` through `cache` and compares their lines
    /// when both are text.
    fn compare(
        &self,
        cache: &mut gix::diff::blob::Platform,
        file: &FileChange,
    ) -> Result<Compared, Error> {
        let failed = |source: Source| Error::Diff {
            path: file.path.clone(),
            source,
        };
        // Submodules on both sides, or one and nothing: no blob to read.
        let blobless = [file.before.as_ref(), file.after.as_ref()]
            .into_iter()
            .flatten()
            .all(|side| side.mode == FileMode::Submodule);
        if blobless {
            return Ok(Compared {
                before: described(file.before.as_ref(), Data::Missing, &file.path)?,
                after: described(file.after.as_ref(), Data::Missing, &file.path)?,
                lines: None,
                hunks: Vec::new(),
                texts: None,
            });
        }
        let before_path = file.from.as_deref().unwrap_or(&file.path);
        let sides = [
            (file.before.as_ref(), before_path, ResourceKind::OldOrSource),
            (
                file.after.as_ref(),
                file.path.as_str(),
                ResourceKind::NewOrDestination,
            ),
        ];
        for (side, path, kind) in sides {
            // A side that is not there, or a submodule's commit, which has
            // no blob, is set as nothing: the cache wants both sides.
            let (id, mode) = match side {
                Some(side) if side.mode != FileMode::Submodule => (oid(&side.id)?, side.mode),
                _ => (
                    gix::ObjectId::null(self.inner.object_hash()),
                    FileMode::File,
                ),
            };
            cache
                .set_resource(
                    id,
                    kind_of(mode),
                    path.as_bytes().as_bstr(),
                    kind,
                    &self.inner.objects,
                )
                .map_err(|e| failed(e.into()))?;
        }
        let outcome = cache.prepare_diff().map_err(|e| failed(e.into()))?;

        let before = described(file.before.as_ref(), outcome.old.data, &file.path)?;
        let after = described(file.after.as_ref(), outcome.new.data, &file.path)?;
        let texts_only = [&before, &after].iter().all(|side| {
            side.as_ref()
                .is_none_or(|s| matches!(s.content, Content::Text { .. }))
        });
        let algorithm = match outcome.operation {
            Operation::InternalDiff { algorithm } if texts_only => Some(algorithm),
            _ => None,
        };
        let Some(algorithm) = algorithm else {
            return Ok(Compared {
                before,
                after,
                lines: None,
                hunks: Vec::new(),
                texts: None,
            });
        };

        let old = outcome.old.data.as_slice().unwrap_or_default();
        let new = outcome.new.data.as_slice().unwrap_or_default();
        let input = InternedInput::new(LineSplit(old), LineSplit(new));
        let mut diff = Diff::compute(algorithm, &input);
        diff.postprocess_lines(&input);
        let hunks = diff
            .hunks()
            .map(|hunk| Hunk {
                before: range(hunk.before),
                after: range(hunk.after),
            })
            .collect();
        let text = |side: &Option<Side>, bytes: &[u8]| {
            side.as_ref()
                .map(|_| String::from_utf8_lossy(bytes).into_owned())
        };
        Ok(Compared {
            lines: Some(LineCount {
                added: diff.count_additions(),
                removed: diff.count_removals(),
            }),
            texts: Some((text(&before, old), text(&after, new))),
            before,
            after,
            hunks,
        })
    }

    /// The bytes of the file at `path` in the commit `revision` names, as
    /// git stores them; `None` when the commit has no file there - nothing
    /// at all, a directory, or a submodule.
    pub fn file_at(&self, revision: &str, path: &str) -> Result<Option<Vec<u8>>, Error> {
        let id = self.resolve(revision)?;
        let Some((entry, mode)) = self.entry_at(id, path)? else {
            return Ok(None);
        };
        if !matches!(mode, FileMode::File | FileMode::Executable | FileMode::Link) {
            return Ok(None);
        }
        let blob = self.inner.find_blob(entry).map_err(|e| Error::Diff {
            path: path.to_owned(),
            source: e.into(),
        })?;
        Ok(Some(blob.data.clone()))
    }

    /// The entry at `path` in commit `id`'s tree, with its kind; `None`
    /// when there is nothing there or it is a directory.
    pub(crate) fn entry_at(
        &self,
        id: gix::ObjectId,
        path: &str,
    ) -> Result<Option<(gix::ObjectId, FileMode)>, Error> {
        let failed = |source: Source| Error::Diff {
            path: path.to_owned(),
            source,
        };
        let tree = self
            .inner
            .find_commit(id)
            .and_then(|commit| commit.tree())
            .map_err(|e| failed(e.into()))?;
        let entry = tree
            .lookup_entry(path.split('/'))
            .map_err(|e| failed(e.into()))?;
        Ok(entry
            .and_then(|entry| mode_of(entry.mode().kind()).map(|mode| (entry.object_id(), mode))))
    }
}

/// Both sides of a change once read, and the lines between them when both
/// are text.
struct Compared {
    before: Option<Side>,
    after: Option<Side>,
    lines: Option<LineCount>,
    hunks: Vec<Hunk>,
    texts: Option<(Option<String>, Option<String>)>,
}

/// A change of the tree diff as a file, its sides not read yet; `None` for a
/// directory, which the files under it stand for.
fn file_change(change: ChangeDetached) -> Option<FileChange> {
    let unread = |id: gix::ObjectId, kind: EntryKind| {
        mode_of(kind).map(|mode| Side {
            id: id.to_string(),
            mode,
            content: Content::Binary,
            size: None,
        })
    };
    let file = |path: &BStr, status, from: Option<&BStr>, similarity, before, after| FileChange {
        path: path.to_string(),
        status,
        from: from.map(ToString::to_string),
        similarity,
        before,
        after,
        lines: None,
    };
    match change {
        ChangeDetached::Addition {
            location,
            entry_mode,
            id,
            ..
        } => {
            let after = unread(id, entry_mode.kind())?;
            Some(file(
                location.as_ref(),
                FileStatus::Added,
                None,
                None,
                None,
                Some(after),
            ))
        }
        ChangeDetached::Deletion {
            location,
            entry_mode,
            id,
            ..
        } => {
            let before = unread(id, entry_mode.kind())?;
            Some(file(
                location.as_ref(),
                FileStatus::Deleted,
                None,
                None,
                Some(before),
                None,
            ))
        }
        ChangeDetached::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
        } => {
            // A directory that became a file is a deletion of what was under
            // it and an addition; only the file is this change's.
            let before = unread(previous_id, previous_entry_mode.kind());
            let after = unread(id, entry_mode.kind());
            let status = match (&before, &after) {
                (Some(_), Some(_)) => FileStatus::Modified,
                (None, Some(_)) => FileStatus::Added,
                (Some(_), None) => FileStatus::Deleted,
                (None, None) => return None,
            };
            Some(file(location.as_ref(), status, None, None, before, after))
        }
        ChangeDetached::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            diff,
            entry_mode,
            id,
            location,
            copy,
            ..
        } => {
            let before = unread(source_id, source_entry_mode.kind())?;
            let after = unread(id, entry_mode.kind())?;
            let status = if copy {
                FileStatus::Copied
            } else {
                FileStatus::Renamed
            };
            // No line statistics means the content did not change at all;
            // otherwise git's figure, which drops the fraction.
            let similarity = diff.map_or(100, |stats| {
                (stats.similarity.clamp(0.0, 1.0) * 100.0).floor() as u8
            });
            Some(file(
                location.as_ref(),
                status,
                Some(source_location.as_ref()),
                Some(similarity),
                Some(before),
                Some(after),
            ))
        }
    }
}

/// A side as the cache read it; `None` when there is no side.
fn described(side: Option<&Side>, data: Data<'_>, path: &str) -> Result<Option<Side>, Error> {
    let Some(side) = side else {
        return Ok(None);
    };
    if side.mode == FileMode::Submodule {
        return Ok(Some(Side {
            content: Content::Submodule,
            size: None,
            ..side.clone()
        }));
    }
    let (content, size) = match data {
        Data::Buffer { buf, .. } => match lfs_pointer(buf) {
            Some((oid, size)) => (Content::Lfs { oid }, size),
            None => (
                Content::Text {
                    endings: endings(buf),
                },
                buf.len() as u64,
            ),
        },
        Data::Binary { size } => (Content::Binary, size),
        Data::Missing => {
            return Err(Error::Diff {
                path: path.to_owned(),
                source: format!("the object {} is not in the repository", side.id).into(),
            });
        }
    };
    Ok(Some(Side {
        content,
        size: Some(size),
        ..side.clone()
    }))
}

/// The object id and size a git LFS pointer names, when `buf` is one.
///
/// A pointer is a few lines of `key value` under a version line, at most
/// 1024 bytes (git-lfs spec v1); the file itself lives in LFS storage.
fn lfs_pointer(buf: &[u8]) -> Option<(String, u64)> {
    if buf.len() > LFS_POINTER_MOST {
        return None;
    }
    let text = std::str::from_utf8(buf).ok()?;
    let mut lines = text.lines();
    let version = lines.next()?.strip_prefix("version ")?;
    if !matches!(
        version,
        "https://git-lfs.github.com/spec/v1" | "https://hawser.github.com/spec/v1"
    ) {
        return None;
    }
    let (mut oid, mut size) = (None, None);
    for line in lines {
        if let Some(hex) = line.strip_prefix("oid sha256:") {
            if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                oid = Some(hex.to_owned());
            }
        } else if let Some(bytes) = line.strip_prefix("size ") {
            size = bytes.parse().ok();
        }
    }
    Some((oid?, size?))
}

/// How the lines of `buf` end.
fn endings(buf: &[u8]) -> Endings {
    let (mut lf, mut crlf) = (false, false);
    for at in buf.find_iter(b"\n") {
        if at > 0 && buf[at - 1] == b'\r' {
            crlf = true;
        } else {
            lf = true;
        }
        if lf && crlf {
            return Endings::Mixed;
        }
    }
    match (lf, crlf) {
        (true, false) => Endings::Lf,
        (false, true) => Endings::Crlf,
        (true, true) => Endings::Mixed,
        (false, false) => Endings::NoEnding,
    }
}

fn mode_of(kind: EntryKind) -> Option<FileMode> {
    match kind {
        EntryKind::Blob => Some(FileMode::File),
        EntryKind::BlobExecutable => Some(FileMode::Executable),
        EntryKind::Link => Some(FileMode::Link),
        EntryKind::Commit => Some(FileMode::Submodule),
        EntryKind::Tree => None,
    }
}

fn kind_of(mode: FileMode) -> EntryKind {
    match mode {
        FileMode::File => EntryKind::Blob,
        FileMode::Executable => EntryKind::BlobExecutable,
        FileMode::Link => EntryKind::Link,
        FileMode::Submodule => EntryKind::Commit,
    }
}

fn oid(hex: &str) -> Result<gix::ObjectId, Error> {
    gix::ObjectId::from_hex(hex.as_bytes()).map_err(|e| Error::Revision {
        revision: hex.to_owned(),
        source: e.into(),
    })
}

fn range(lines: std::ops::Range<u32>) -> LineRange {
    LineRange {
        start: lines.start,
        count: lines.end - lines.start,
    }
}

/// Lines split at `\n`, which is left off. A `\r` before it stays part of
/// the line, so a line that only changed its ending is a changed line, as
/// git shows it.
#[derive(Clone, Copy)]
struct LineSplit<'a>(&'a [u8]);

impl<'a> Iterator for LineSplit<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        let (line, rest) = match self.0.find_byte(b'\n') {
            Some(end) => (&self.0[..end], &self.0[end + 1..]),
            None => (self.0, &b""[..]),
        };
        self.0 = rest;
        Some(line)
    }
}

impl<'a> TokenSource for LineSplit<'a> {
    type Token = &'a [u8];
    type Tokenizer = Self;

    fn tokenize(&self) -> Self::Tokenizer {
        *self
    }

    fn estimate_tokens(&self) -> u32 {
        // A guess from the first lines, as imara-diff's own sources make it.
        let sample: usize = self.take(20).map(<[u8]>::len).sum::<usize>() + 20;
        u32::try_from(self.0.len() * 20 / sample).unwrap_or(u32::MAX)
    }
}

impl FileDiff {
    /// The change as a patch reads, with `context` unchanged lines around
    /// each hunk; hunks that close in on each other are one, as in `git
    /// diff`. A side that is not text gives a patch without hunks.
    pub fn patch(&self, context: u32) -> Patch {
        let before: Vec<&str> = self.before.as_deref().map(split).unwrap_or_default();
        let after: Vec<&str> = self.after.as_deref().map(split).unwrap_or_default();
        let mut hunks: Vec<PatchHunk> = Vec::new();
        // The hunks of the diff that each patch hunk joins.
        let mut groups: Vec<Vec<Hunk>> = Vec::new();
        for hunk in &self.hunks {
            let joins = groups
                .last()
                .and_then(|group| group.last())
                .is_some_and(|last| {
                    hunk.before.start <= last.before.start + last.before.count + 2 * context
                });
            if joins {
                groups.last_mut().expect("a group to join").push(*hunk);
            } else {
                groups.push(vec![*hunk]);
            }
        }
        for group in groups {
            let first = group[0];
            let last = group[group.len() - 1];
            // Hunks further apart than twice the context are patch hunks of
            // their own, so the lines around one are unchanged on both sides.
            let lead = context.min(first.before.start);
            let before_end =
                (last.before.start + last.before.count + context).min(before.len() as u32);
            let mut lines = Vec::new();
            let mut at = first.before.start - lead;
            for hunk in &group {
                for line in at..hunk.before.start {
                    lines.push(PatchLine {
                        kind: LineKind::Context,
                        text: before[line as usize].to_owned(),
                    });
                }
                for line in hunk.before.start..hunk.before.start + hunk.before.count {
                    lines.push(PatchLine {
                        kind: LineKind::Removed,
                        text: before[line as usize].to_owned(),
                    });
                }
                for line in hunk.after.start..hunk.after.start + hunk.after.count {
                    lines.push(PatchLine {
                        kind: LineKind::Added,
                        text: after[line as usize].to_owned(),
                    });
                }
                at = hunk.before.start + hunk.before.count;
            }
            for line in at..before_end {
                lines.push(PatchLine {
                    kind: LineKind::Context,
                    text: before[line as usize].to_owned(),
                });
            }
            let before_start = first.before.start - lead;
            let after_start = first.after.start - lead;
            let span = |kind: LineKind| lines.iter().filter(|l| l.kind == kind).count() as u32;
            let context_lines = span(LineKind::Context);
            hunks.push(PatchHunk {
                before: LineRange {
                    start: before_start,
                    count: context_lines + span(LineKind::Removed),
                },
                after: LineRange {
                    start: after_start,
                    count: context_lines + span(LineKind::Added),
                },
                lines,
            });
        }
        Patch {
            file: self.file.clone(),
            hunks,
        }
    }
}

/// A text's lines as the diff counted them: split at `\n`, which is left
/// off, with nothing after a final newline.
fn split(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if text.is_empty() || text.ends_with('\n') {
        lines.pop();
    }
    lines
}
