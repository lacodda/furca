//! The working copy against `HEAD`: what is staged, what is not, what git
//! does not track, and what a merge left unresolved.
//!
//! Listing it and reading it are two steps. [`Repository::status`] says which
//! files differ and how, without opening one: that is what has to be instant,
//! after every save. What each file holds - text, a binary, a git LFS pointer -
//! and how many lines moved come after, from [`StatusCache::read_files`], the
//! same reading a commit's changes get. A formatter run across a repository
//! changes thousands of files; the list is there at once and the sizes fill in.
//!
//! [`StatusCache`] is what keeps a save cheap. While `HEAD` and the index stay
//! as they were, a change to one file can only move that file's row, so only
//! the paths something names are looked at again. Nothing here writes: git
//! itself refreshes the index's timestamps during `git status`, this does not
//! (ADR 0001, ADR 0014).

use std::collections::{HashMap, HashSet};

use gix::bstr::{BString, ByteSlice};
use gix::diff::index::ChangeRef;
use gix::dir::entry::{Kind, Status as WalkStatus};
use gix::index::entry::Mode;
use gix::status::Item;
use gix::status::index_worktree::Item as WorktreeItem;
use gix::status::plumbing::index_as_worktree::{Change, Conflict, EntryStatus};
use serde::Serialize;

use crate::Repository;
use crate::diff::{Content, FileChange, FileMode, FileStatus, Side};
use crate::error::{Error, Source, wrap};

/// More touched paths than this are read as a whole: past a few hundred,
/// matching every index entry against every path costs more than the walk.
const MOST_PATHS: usize = 512;

/// The working copy against `HEAD`, each list by path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Status {
    /// What the next commit would change: `HEAD` against the index.
    pub staged: Vec<FileChange>,
    /// What the index does not have: the index against the working tree.
    pub unstaged: Vec<FileChange>,
    /// Files git does not track and does not ignore, each one on its own, as
    /// `git status --untracked-files=all` lists them. Empty when
    /// `status.showUntrackedFiles` is `no`.
    pub untracked: Vec<FileChange>,
    /// Paths a merge left unresolved.
    pub conflicted: Vec<Conflicted>,
}

impl Status {
    /// Whether the working copy is exactly `HEAD`, as far as git can tell.
    pub fn is_clean(&self) -> bool {
        self.staged.is_empty()
            && self.unstaged.is_empty()
            && self.untracked.is_empty()
            && self.conflicted.is_empty()
    }

    fn sort(&mut self) {
        for list in [&mut self.staged, &mut self.unstaged, &mut self.untracked] {
            list.sort_by(|a, b| a.path.cmp(&b.path));
        }
        self.conflicted.sort_by(|a, b| a.path.cmp(&b.path));
    }
}

/// A path a merge left unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Conflicted {
    pub path: String,
    pub kind: ConflictKind,
}

/// How two sides of a merge disagree about a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum ConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
}

impl Repository {
    /// Lists the working copy: every file that differs, and how, with no file
    /// opened. A side of each change is [`Content::Unread`] until
    /// [`Repository::diff`] or [`StatusCache::read_files`] reads it.
    ///
    /// A repository without commits has every indexed file staged as added;
    /// a bare repository has no working copy, and an empty status.
    pub fn status(&self) -> Result<Status, Error> {
        let mut status = Status::default();
        if self.workdir().is_none() {
            return Ok(status);
        }
        let items = self
            .status_platform()?
            .into_iter(Vec::new())
            .map_err(wrap(Error::Status))?;
        for item in items {
            match item.map_err(wrap(Error::Status))? {
                Item::TreeIndex(change) => status.staged.push(staged_change(change)),
                Item::IndexWorktree(item) => take_worktree_item(item, &mut status),
            }
        }
        status.sort();
        Ok(status)
    }

    fn status_platform(&self) -> Result<gix::status::Platform<'_, gix::progress::Discard>, Error> {
        Ok(self
            .inner
            .status(gix::progress::Discard)
            .map_err(wrap(Error::Status))?
            // Each file on its own. A repository whose `status.showUntrackedFiles`
            // is `no` has had the walk taken away already, and this leaves it so.
            .untracked_files(gix::status::UntrackedFiles::Files)
            // A file moved in the working tree reads as one deleted and one
            // untracked until it is staged, as `git status` shows it.
            .index_worktree_rewrites(None)
            .index_worktree_submodules(gix::status::Submodule::AsConfigured { check_dirty: true }))
    }
}

/// What changed in the list after [`StatusCache::refresh`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "kind", content = "paths", rename_all = "kebab-case")]
pub enum Refreshed {
    /// No row moved: what changed is ignored, or changed nothing git sees.
    Nothing,
    /// These paths were read again, and whatever is shown of them is stale.
    Files(Vec<String>),
    /// The whole list was read again.
    Everything,
}

/// The working copy as last read, kept up to date by the paths something says
/// changed. See the module's notes.
#[derive(Debug, Clone)]
pub struct StatusCache {
    status: Status,
    stamp: Stamp,
    /// The files that could not be read, so a pass over the rest ends: a
    /// filter that fails, a file gone since it was listed. Opening one says
    /// why. A later change to the path tries again.
    failed: HashSet<(Area, String)>,
}

/// What a whole read of the status depends on, other than the files.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    head: Option<gix::ObjectId>,
    /// The index's own checksum; `None` when it has none to compare, which
    /// is never taken to mean the same index.
    index: Option<gix::ObjectId>,
}

impl Stamp {
    fn of(repo: &Repository) -> Result<Self, Error> {
        let index = repo.inner.index_or_empty().map_err(wrap(Error::Index))?;
        Ok(Self {
            head: repo.inner.head_id().ok().map(|id| id.detach()),
            index: index.checksum().filter(|id| !id.is_null()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Area {
    Staged,
    Unstaged,
    Untracked,
}

impl StatusCache {
    /// Reads the whole status.
    pub fn read(repo: &Repository) -> Result<Self, Error> {
        // The stamp before the read: a change between the two reads as a
        // change next time, never as nothing.
        let stamp = Stamp::of(repo)?;
        Ok(Self {
            status: repo.status()?,
            stamp,
            failed: HashSet::new(),
        })
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Reads the whole status again: when something may have been missed.
    pub fn read_again(&mut self, repo: &Repository) -> Result<(), Error> {
        let stamp = Stamp::of(repo)?;
        let mut fresh = repo.status()?;
        // A staged change is two objects, which do not change: what was read
        // of them still holds. A file on disk may have changed unseen.
        let read: HashMap<_, _> = self
            .status
            .staged
            .drain(..)
            .map(|file| (identity(&file), file))
            .collect();
        for file in &mut fresh.staged {
            if let Some(kept) = read.get(&identity(file)) {
                *file = kept.clone();
            }
        }
        self.status = fresh;
        self.stamp = stamp;
        self.failed.clear();
        Ok(())
    }

    /// Brings the list up to date after `paths` changed in the working tree -
    /// paths from its root with `/` separators, files or folders, as the
    /// watcher names them.
    ///
    /// While `HEAD` and the index are what they were, only those paths are
    /// looked at. Otherwise, and when a path changes what git ignores or how
    /// it reads files, the whole status is read.
    pub fn refresh(&mut self, repo: &Repository, paths: &[String]) -> Result<Refreshed, Error> {
        let stamp = Stamp::of(repo)?;
        if stamp != self.stamp
            || stamp.index.is_none()
            || paths.iter().any(|path| changes_the_rules(path))
        {
            self.read_again(repo)?;
            return Ok(Refreshed::Everything);
        }
        let paths = self.worth_looking_at(repo, paths)?;
        if paths.is_empty() {
            return Ok(Refreshed::Nothing);
        }
        if paths.len() > MOST_PATHS {
            self.read_again(repo)?;
            return Ok(Refreshed::Everything);
        }

        let literal: Vec<BString> = paths
            .iter()
            .map(|path| format!(":(top,literal){path}").into())
            .collect();
        let items = repo
            .status_platform()?
            .into_index_worktree_iter(literal)
            .map_err(wrap(Error::Status))?;
        let mut fresh = Status::default();
        for item in items {
            take_worktree_item(item.map_err(wrap(Error::Status))?, &mut fresh);
        }

        let touched = |path: &str| {
            paths.iter().any(|at| {
                path == at
                    || path
                        .strip_prefix(at.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        };
        let mut moved: Vec<String> = Vec::new();
        for (area, list) in [
            (Area::Unstaged, &mut self.status.unstaged),
            (Area::Untracked, &mut self.status.untracked),
        ] {
            let failed = &mut self.failed;
            list.retain(|file| {
                let stays = !touched(&file.path);
                if !stays {
                    failed.remove(&(area, file.path.clone()));
                    moved.push(file.path.clone());
                }
                stays
            });
        }
        moved.extend(fresh.unstaged.iter().map(|file| file.path.clone()));
        moved.extend(fresh.untracked.iter().map(|file| file.path.clone()));
        self.status.unstaged.append(&mut fresh.unstaged);
        self.status.untracked.append(&mut fresh.untracked);
        // Conflicted paths live in the index, which has not changed; the only
        // ones the scan can name are the ones already listed.
        self.status.sort();

        moved.sort();
        moved.dedup();
        self.stamp = stamp;
        Ok(if moved.is_empty() {
            Refreshed::Nothing
        } else {
            Refreshed::Files(moved)
        })
    }

    /// The paths that can move a row: not what git ignores, unless the index
    /// tracks it anyway. A build writes thousands of files into an ignored
    /// folder, and none of them is worth a look. What is inside a submodule
    /// is the submodule's: its path stands for it here.
    fn worth_looking_at(&self, repo: &Repository, paths: &[String]) -> Result<Vec<String>, Error> {
        let index = repo.inner.index_or_empty().map_err(wrap(Error::Index))?;
        let mut excludes = repo
            .inner
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(wrap(Error::Status))?;
        let mut kept = Vec::new();
        for path in paths {
            if let Some(submodule) = submodule_around(&index, path) {
                kept.push(submodule.to_owned());
                continue;
            }
            let bytes = path.as_bytes().as_bstr();
            let tracked = index.entry_by_path(bytes).is_some()
                || index
                    .prefixed_entries(format!("{path}/").as_bytes().as_bstr())
                    .is_some_and(|entries| !entries.is_empty());
            if tracked {
                kept.push(path.clone());
                continue;
            }
            let ignored = excludes
                .at_entry(bytes, None)
                .map_err(wrap(Error::Status))?
                .is_excluded();
            if !ignored {
                kept.push(path.clone());
            }
        }
        kept.sort();
        kept.dedup();
        Ok(kept)
    }

    /// Whether any file is still to be read.
    pub fn has_unread(&self) -> bool {
        self.has_unread_in(usize::MAX)
    }

    /// Whether any file among the first `rows` of a list is still to be read.
    pub fn has_unread_in(&self, rows: usize) -> bool {
        let lists = [
            (Area::Staged, &self.status.staged),
            (Area::Unstaged, &self.status.unstaged),
            (Area::Untracked, &self.status.untracked),
        ];
        lists.iter().any(|(area, list)| {
            list.iter()
                .take(rows)
                .any(|file| is_unread(file) && !self.failed.contains(&(*area, file.path.clone())))
        })
    }

    /// Reads what up to `most` files hold, and their lines; returns how many
    /// were looked at, so 0 means nothing is left. A file that cannot be read
    /// stays unread and is not tried again until it changes: one broken
    /// filter does not stop the list, and opening the file says what is
    /// wrong.
    pub fn read_files(&mut self, repo: &Repository, most: usize) -> usize {
        self.read_rows(repo, most, usize::MAX)
    }

    /// The same among the first `rows` of each list: what is past the rows
    /// anyone is shown need not be read, and a formatter's eight thousand files
    /// cost what the screen shows of them.
    pub fn read_rows(&mut self, repo: &Repository, most: usize, rows: usize) -> usize {
        let mut read = 0;
        let lists = [
            (Area::Staged, &mut self.status.staged),
            (Area::Unstaged, &mut self.status.unstaged),
            (Area::Untracked, &mut self.status.untracked),
        ];
        for (area, list) in lists {
            let mut cache: Option<Result<gix::diff::blob::Platform, Source>> = None;
            for file in list.iter_mut().take(rows) {
                if read == most {
                    return read;
                }
                if !is_unread(file) || self.failed.contains(&(area, file.path.clone())) {
                    continue;
                }
                read += 1;
                let cache = cache.get_or_insert_with(|| match area {
                    Area::Staged => repo
                        .inner
                        .diff_resource_cache_for_tree_diff()
                        .map_err(Into::into),
                    Area::Unstaged | Area::Untracked => repo.working_tree_cache(),
                });
                let compared = match cache {
                    Ok(cache) => {
                        let compared = repo.compare(cache, file);
                        // The buffers are done with once the file is read; a
                        // long list would otherwise hold every one of them.
                        cache.clear_resource_cache_keep_allocation();
                        compared.ok()
                    }
                    Err(_) => None,
                };
                match compared {
                    Some(compared) => drop(compared.apply_to(file)),
                    None => {
                        self.failed.insert((area, file.path.clone()));
                    }
                }
            }
        }
        read
    }
}

/// What identifies a staged change, whatever was read of it.
fn identity(file: &FileChange) -> (String, Option<String>, Option<String>) {
    let id = |side: &Option<Side>| side.as_ref().and_then(|side| side.id.clone());
    (file.path.clone(), id(&file.before), id(&file.after))
}

fn is_unread(file: &FileChange) -> bool {
    [&file.before, &file.after]
        .into_iter()
        .flatten()
        .any(|side| side.content == Content::Unread)
}

/// Whether a change to `path` can change the status of files other than
/// itself: what git ignores, how it reads a file, what is a submodule.
fn changes_the_rules(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(name, ".gitignore" | ".gitattributes" | ".gitmodules")
}

/// The submodule `path` is inside, if any.
fn submodule_around<'a>(index: &gix::index::File, path: &'a str) -> Option<&'a str> {
    path.match_indices('/').find_map(|(at, _)| {
        let parent = &path[..at];
        index
            .entry_by_path(parent.as_bytes().as_bstr())
            .is_some_and(|entry| entry.mode == Mode::COMMIT)
            .then_some(parent)
    })
}

fn file_mode(mode: Mode) -> FileMode {
    if mode == Mode::FILE_EXECUTABLE {
        FileMode::Executable
    } else if mode == Mode::SYMLINK {
        FileMode::Link
    } else if mode == Mode::COMMIT {
        FileMode::Submodule
    } else {
        FileMode::File
    }
}

/// A side git has stored.
fn stored(id: &gix::oid, mode: Mode) -> Side {
    Side {
        id: Some(id.to_string()),
        mode: file_mode(mode),
        content: Content::Unread,
        size: None,
    }
}

/// A side only the working tree has.
fn on_disk(mode: FileMode) -> Side {
    Side {
        id: None,
        mode,
        content: Content::Unread,
        size: None,
    }
}

fn change(
    path: String,
    status: FileStatus,
    before: Option<Side>,
    after: Option<Side>,
) -> FileChange {
    FileChange {
        path,
        status,
        from: None,
        similarity: None,
        before,
        after,
        lines: None,
    }
}

/// A change between `HEAD` and the index as a file.
fn staged_change(item: gix::diff::index::Change) -> FileChange {
    match item {
        ChangeRef::Addition {
            location,
            entry_mode,
            id,
            ..
        } => change(
            location.to_string(),
            FileStatus::Added,
            None,
            Some(stored(&id, entry_mode)),
        ),
        ChangeRef::Deletion {
            location,
            entry_mode,
            id,
            ..
        } => change(
            location.to_string(),
            FileStatus::Deleted,
            Some(stored(&id, entry_mode)),
            None,
        ),
        ChangeRef::Modification {
            location,
            previous_entry_mode,
            previous_id,
            entry_mode,
            id,
            ..
        } => change(
            location.to_string(),
            FileStatus::Modified,
            Some(stored(&previous_id, previous_entry_mode)),
            Some(stored(&id, entry_mode)),
        ),
        ChangeRef::Rewrite {
            source_location,
            source_entry_mode,
            source_id,
            location,
            entry_mode,
            id,
            copy,
            ..
        } => {
            let same = source_id == id;
            FileChange {
                from: Some(source_location.to_string()),
                // The index diff gives no figure; reading the two gives it.
                similarity: same.then_some(100),
                ..change(
                    location.to_string(),
                    if copy {
                        FileStatus::Copied
                    } else {
                        FileStatus::Renamed
                    },
                    Some(stored(&source_id, source_entry_mode)),
                    Some(stored(&id, entry_mode)),
                )
            }
        }
    }
}

/// Puts a difference between the index and the working tree where it belongs.
fn take_worktree_item(item: WorktreeItem, into: &mut Status) {
    match item {
        WorktreeItem::Modification {
            entry,
            rela_path,
            status,
            ..
        } => {
            let path = rela_path.to_string();
            let before = stored(&entry.id, entry.mode);
            match status {
                EntryStatus::Conflict { summary, .. } => into.conflicted.push(Conflicted {
                    path,
                    kind: conflict_kind(summary),
                }),
                // Nothing changed; only the index's timestamps are behind.
                EntryStatus::NeedsUpdate(_) => {}
                EntryStatus::IntentToAdd => into.unstaged.push(change(
                    path,
                    FileStatus::Added,
                    None,
                    Some(on_disk(file_mode(entry.mode))),
                )),
                EntryStatus::Change(Change::Removed) => {
                    into.unstaged
                        .push(change(path, FileStatus::Deleted, Some(before), None))
                }
                EntryStatus::Change(Change::Type { worktree_mode }) => into.unstaged.push(change(
                    path,
                    FileStatus::Modified,
                    Some(before),
                    Some(on_disk(file_mode(worktree_mode))),
                )),
                EntryStatus::Change(Change::Modification {
                    executable_bit_changed,
                    ..
                }) => {
                    let mode = match (file_mode(entry.mode), executable_bit_changed) {
                        (FileMode::File, true) => FileMode::Executable,
                        (FileMode::Executable, true) => FileMode::File,
                        (mode, _) => mode,
                    };
                    into.unstaged.push(change(
                        path,
                        FileStatus::Modified,
                        Some(before),
                        Some(on_disk(mode)),
                    ));
                }
                EntryStatus::Change(Change::SubmoduleModification(submodule)) => {
                    let after = Side {
                        id: submodule
                            .checked_out_head_id
                            .map(|id| id.to_string())
                            .or_else(|| before.id.clone()),
                        ..before.clone()
                    };
                    into.unstaged.push(change(
                        path,
                        FileStatus::Modified,
                        Some(before),
                        Some(after),
                    ));
                }
            }
        }
        WorktreeItem::DirectoryContents { entry, .. } => {
            if entry.status != WalkStatus::Untracked {
                return;
            }
            let mode = match entry.disk_kind {
                Some(Kind::File) => FileMode::File,
                Some(Kind::Symlink) => FileMode::Link,
                // A repository inside this one, which git lists as one entry
                // and would add as a submodule.
                Some(Kind::Repository) => FileMode::Submodule,
                _ => return,
            };
            into.untracked.push(change(
                entry.rela_path.to_string(),
                FileStatus::Added,
                None,
                Some(on_disk(mode)),
            ));
        }
        // Tracking is off in `status_platform`.
        WorktreeItem::Rewrite { .. } => {}
    }
}

fn conflict_kind(conflict: Conflict) -> ConflictKind {
    match conflict {
        Conflict::BothDeleted => ConflictKind::BothDeleted,
        Conflict::AddedByUs => ConflictKind::AddedByUs,
        Conflict::DeletedByThem => ConflictKind::DeletedByThem,
        Conflict::AddedByThem => ConflictKind::AddedByThem,
        Conflict::DeletedByUs => ConflictKind::DeletedByUs,
        Conflict::BothAdded => ConflictKind::BothAdded,
        Conflict::BothModified => ConflictKind::BothModified,
    }
}
