//! Hearing a working copy change: the file system's events on the working tree
//! and the git directory, sorted into what they mean and gathered into
//! batches, so a save - a temporary file, a rename, a touch of the folder -
//! is one thing to read, and a build's thousands of writes a few.
//!
//! An event is a hint, not a fact. What a batch names is read again by
//! [`StatusCache::refresh`](crate::StatusCache::refresh), which decides what
//! actually moved and drops what git ignores; a file system that loses events
//! says so, and the batch then asks for everything. Behind the `watch` feature:
//! a library user who only reads gets no new dependency.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::Repository;
use crate::error::Error;

/// How long the events have to pause before a batch is handed over. Longer than
/// the steps of one save, short enough not to be seen.
const QUIET: Duration = Duration::from_millis(60);
/// The longest a batch waits under events that never pause, as during a build.
const AT_MOST: Duration = Duration::from_millis(400);

/// What changed, gathered over a moment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    /// Files and folders of the working tree that changed, from its root with
    /// `/` separators, in order. Ignored files are among them: telling them
    /// apart is the status's job, with the rules as they stand when it reads.
    pub paths: Vec<String>,
    /// The index was written: something was staged, committed, checked out.
    pub index: bool,
    /// `HEAD`, a ref or an operation under way moved.
    pub refs: bool,
    /// Something may have been missed, or the configuration changed: read
    /// everything again.
    pub everything: bool,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.index && !self.refs && !self.everything
    }
}

/// Hears a repository's working copy until it is dropped.
pub struct Watcher {
    // Dropping it stops the events, which ends the thread gathering them.
    _notify: RecommendedWatcher,
}

impl Repository {
    /// Starts watching the working tree and the git directory, and calls
    /// `on_batch` on a thread of its own with what changes, until the returned
    /// [`Watcher`] is dropped.
    pub fn watch(&self, on_batch: impl FnMut(Batch) + Send + 'static) -> Result<Watcher, Error> {
        let places = Places::of(self)?;
        let (events, heard) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            // Sent to a thread that has gone only once the watcher is dropped.
            let _ = events.send(event);
        })
        .map_err(|e| Error::Watch(e.into()))?;
        for root in places.roots() {
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|e| Error::Watch(e.into()))?;
        }
        std::thread::Builder::new()
            .name("furca-watch".to_owned())
            .spawn(move || gather(&heard, &places, on_batch))
            .map_err(|e| Error::Watch(e.into()))?;
        Ok(Watcher { _notify: watcher })
    }
}

/// Where a repository's files are, in the form the file system reports them.
struct Places {
    workdir: Option<PathBuf>,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl Places {
    fn of(repo: &Repository) -> Result<Self, Error> {
        let real = |path: &Path| std::fs::canonicalize(path).map_err(|e| Error::Watch(Box::new(e)));
        Ok(Self {
            workdir: repo.workdir().map(real).transpose()?,
            git_dir: real(repo.git_dir())?,
            common_dir: real(repo.inner.common_dir())?,
        })
    }

    /// The folders to watch: the working tree, and the git directories that
    /// are not inside it - a linked worktree's are elsewhere.
    fn roots(&self) -> Vec<&Path> {
        let mut roots: Vec<&Path> = self.workdir.iter().map(PathBuf::as_path).collect();
        for dir in [&self.git_dir, &self.common_dir] {
            if !roots.iter().any(|root| dir.starts_with(root)) {
                roots.push(dir);
            }
        }
        roots
    }

    fn place(&self, path: &Path) -> Place {
        for dir in [&self.git_dir, &self.common_dir] {
            if let Ok(inside) = path.strip_prefix(dir) {
                return git_place(inside);
            }
        }
        match self.workdir.as_ref().map(|root| path.strip_prefix(root)) {
            Some(Ok(inside)) => working_tree_place(inside),
            _ => Place::Nothing,
        }
    }
}

/// What a changed path means.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    Nothing,
    Path(String),
    Index,
    Refs,
    Everything,
}

/// A path inside a git directory, from its root. Lock files come and go on
/// every write; the rename onto the real name is the event that counts.
fn git_place(inside: &Path) -> Place {
    let inside = inside.to_string_lossy().replace('\\', "/");
    if inside.ends_with(".lock") {
        return Place::Nothing;
    }
    let first = inside.split('/').next().unwrap_or_default();
    match first {
        "index" if inside == "index" => Place::Index,
        "HEAD" | "packed-refs" | "refs" | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD"
        | "BISECT_LOG" | "rebase-merge" | "rebase-apply" => Place::Refs,
        // Both read when a status starts: `core.excludesFile`, what to show.
        "config" if inside == "config" => Place::Everything,
        "info" if inside == "info/exclude" => Place::Everything,
        _ => Place::Nothing,
    }
}

/// A path inside the working tree, from its root. What is in another
/// repository's git directory - a submodule's, a nested one's - is that
/// repository's business.
fn working_tree_place(inside: &Path) -> Place {
    let mut parts = Vec::new();
    for component in inside.components() {
        let Component::Normal(part) = component else {
            return Place::Nothing;
        };
        // A name that is not UTF-8 cannot be named in a batch.
        let Some(part) = part.to_str() else {
            return Place::Everything;
        };
        if part.eq_ignore_ascii_case(".git") {
            return Place::Nothing;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        Place::Nothing
    } else {
        Place::Path(parts.join("/"))
    }
}

/// An event that only says a folder's own timestamps moved: writing inside a
/// folder touches it, and on Windows that is reported on its own. Git sees no
/// folders, and the changes inside are reported as themselves; reading the
/// folder would read everything under it for nothing.
fn only_touches_a_folder(kind: &EventKind, path: &Path) -> bool {
    matches!(
        kind,
        EventKind::Modify(
            ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Other
        )
    ) && path.is_dir()
}

/// A batch being gathered, with no clock of its own: time is handed in, which
/// is what lets a test run it.
#[derive(Debug, Default)]
struct Gathering {
    paths: BTreeSet<String>,
    index: bool,
    refs: bool,
    everything: bool,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Gathering {
    fn add(&mut self, place: Place, now: Instant) {
        match place {
            Place::Nothing => return,
            Place::Path(path) => {
                self.paths.insert(path);
            }
            Place::Index => self.index = true,
            Place::Refs => self.refs = true,
            Place::Everything => self.everything = true,
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// How long until the batch is due; `None` when nothing is waiting.
    fn due_in(&self, now: Instant) -> Option<Duration> {
        let (first, last) = (self.first?, self.last?);
        let quiet = last + QUIET;
        let latest = first + AT_MOST;
        Some(quiet.min(latest).saturating_duration_since(now))
    }

    fn take(&mut self) -> Batch {
        let taken = std::mem::take(self);
        Batch {
            paths: taken.paths.into_iter().collect(),
            index: taken.index,
            refs: taken.refs,
            everything: taken.everything,
        }
    }
}

/// Gathers events into batches until the watcher is dropped.
fn gather(
    heard: &Receiver<notify::Result<notify::Event>>,
    places: &Places,
    mut on_batch: impl FnMut(Batch),
) {
    let mut gathering = Gathering::default();
    loop {
        let wait = gathering.due_in(Instant::now());
        let event = match wait {
            None => heard.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(wait) => heard.recv_timeout(wait),
        };
        match event {
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                let batch = gathering.take();
                if !batch.is_empty() {
                    on_batch(batch);
                }
            }
            Ok(event) => {
                let now = Instant::now();
                match event {
                    // What the file system could not report it reports having
                    // lost: every event since is only a guess.
                    Err(_) => gathering.add(Place::Everything, now),
                    Ok(event) if event.need_rescan() => gathering.add(Place::Everything, now),
                    // A read is not a change, and our own reads would never end.
                    Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
                    Ok(event) => {
                        for path in &event.paths {
                            if !only_touches_a_folder(&event.kind, path) {
                                gathering.add(places.place(path), now);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn a_save_in_several_steps_is_one_batch() {
        let start = Instant::now();
        let mut gathering = Gathering::default();
        assert_eq!(gathering.due_in(start), None, "nothing is waiting");

        gathering.add(Place::Path("a.tmp".into()), at(start, 0));
        gathering.add(Place::Path("a.rs".into()), at(start, 5));
        gathering.add(Place::Path("a.rs".into()), at(start, 10));
        assert_eq!(gathering.due_in(at(start, 10)), Some(QUIET));
        assert_eq!(gathering.due_in(at(start, 500)), Some(Duration::ZERO));

        let batch = gathering.take();
        assert_eq!(batch.paths, ["a.rs", "a.tmp"]);
        assert_eq!(gathering.due_in(at(start, 500)), None);
    }

    #[test]
    fn events_that_never_pause_are_handed_over_all_the_same() {
        let start = Instant::now();
        let mut gathering = Gathering::default();
        for millis in (0..1_000).step_by(10) {
            gathering.add(Place::Path("target/x".into()), at(start, millis));
            let wait = gathering.due_in(at(start, millis));
            if millis >= AT_MOST.as_millis() as u64 {
                assert_eq!(wait, Some(Duration::ZERO), "at {millis} ms");
            } else {
                assert!(wait.is_some_and(|wait| wait <= QUIET), "at {millis} ms");
            }
        }
    }

    #[test]
    fn nothing_worth_saying_starts_no_batch() {
        let start = Instant::now();
        let mut gathering = Gathering::default();
        gathering.add(Place::Nothing, start);
        assert_eq!(gathering.due_in(start), None);
        assert!(gathering.take().is_empty());
    }

    #[test]
    fn a_folder_is_left_out_only_when_all_that_happened_was_a_touch() {
        use notify::event::{CreateKind, DataChange, MetadataKind, RenameMode};

        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "a").expect("write");
        let touched = [
            EventKind::Modify(ModifyKind::Any),
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)),
        ];
        for kind in touched {
            assert!(only_touches_a_folder(&kind, dir.path()), "{kind:?}");
            assert!(!only_touches_a_folder(&kind, &file), "a file: {kind:?}");
        }
        // A folder moved in or made is all the events there will be of what
        // is inside it.
        for kind in [
            EventKind::Create(CreateKind::Folder),
            EventKind::Modify(ModifyKind::Name(RenameMode::To)),
        ] {
            assert!(!only_touches_a_folder(&kind, dir.path()), "{kind:?}");
        }
    }

    #[test]
    fn the_git_directory_says_what_moved_not_every_write() {
        let place = |path: &str| git_place(Path::new(path));
        assert_eq!(place("index"), Place::Index);
        assert_eq!(place("index.lock"), Place::Nothing);
        assert_eq!(place("HEAD"), Place::Refs);
        assert_eq!(place("HEAD.lock"), Place::Nothing);
        assert_eq!(place("packed-refs"), Place::Refs);
        assert_eq!(place("refs/heads/feature/x"), Place::Refs);
        assert_eq!(place("refs/heads/main.lock"), Place::Nothing);
        assert_eq!(place("rebase-merge/msgnum"), Place::Refs);
        assert_eq!(place("config"), Place::Everything);
        assert_eq!(place("info/exclude"), Place::Everything);
        assert_eq!(place("info/refs"), Place::Nothing);
        assert_eq!(place("objects/ab/cdef"), Place::Nothing);
        assert_eq!(place("logs/HEAD"), Place::Nothing);
        assert_eq!(place("COMMIT_EDITMSG"), Place::Nothing);
        assert_eq!(place(""), Place::Nothing);
    }

    #[test]
    fn the_working_tree_names_paths_with_forward_slashes() {
        let place = |path: &str| working_tree_place(Path::new(path));
        assert_eq!(place("src/lib.rs"), Place::Path("src/lib.rs".into()));
        assert_eq!(place("a file.txt"), Place::Path("a file.txt".into()));
        assert_eq!(place(".gitignore"), Place::Path(".gitignore".into()));
        assert_eq!(place(".git"), Place::Nothing, "a linked worktree's file");
        assert_eq!(place("nested/.git/HEAD"), Place::Nothing);
        assert_eq!(place(""), Place::Nothing);
    }
}
