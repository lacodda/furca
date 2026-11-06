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
//!
//! Windows and macOS watch a tree whole. On Linux that is one inotify watch per
//! folder, ignored ones included, and a `node_modules` is enough to reach the
//! system's limit; so there the folders are walked, what git ignores is left out,
//! and each of the rest is watched on its own (ADR 0016).

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use gix::bstr::ByteSlice;
use notify::event::{EventKind, ModifyKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::Repository;
use crate::error::{Error, wrap};

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
    /// Why a folder that appeared could not be watched: from here on some of
    /// the working tree goes unheard.
    pub unwatched: Option<String>,
}

impl Batch {
    fn is_empty(&self) -> bool {
        self.paths.is_empty()
            && !self.index
            && !self.refs
            && !self.everything
            && self.unwatched.is_none()
    }
}

/// Hears a repository's working copy until it is dropped.
pub struct Watcher {
    // The only strong hold on it. Dropping it stops the events, which ends the
    // thread gathering them.
    _notify: Arc<Mutex<RecommendedWatcher>>,
}

/// How a tree is watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    /// By the file system, whole.
    Whole,
    /// Folder by folder, what git ignores left out.
    Walked,
}

const WAY: Way = if cfg!(target_os = "linux") {
    Way::Walked
} else {
    Way::Whole
};

impl Repository {
    /// Starts watching the working tree and the git directory, and calls
    /// `on_batch` on a thread of its own with what changes, until the returned
    /// [`Watcher`] is dropped.
    pub fn watch(&self, on_batch: impl FnMut(Batch) + Send + 'static) -> Result<Watcher, Error> {
        self.watch_the_way(WAY, on_batch)
    }

    fn watch_the_way(
        &self,
        way: Way,
        on_batch: impl FnMut(Batch) + Send + 'static,
    ) -> Result<Watcher, Error> {
        let places = Places::of(self)?;
        let (events, heard) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            // Sent to a thread that has gone only once the watcher is dropped.
            let _ = events.send(event);
        })
        .map_err(|e| Error::Watch(e.into()))?;
        let watcher = Arc::new(Mutex::new(watcher));
        let walker = match way {
            Way::Whole => {
                let mut whole = watcher.lock().map_err(|_| poisoned())?;
                for root in places.roots() {
                    whole
                        .watch(root, RecursiveMode::Recursive)
                        .map_err(|e| Error::Watch(e.into()))?;
                }
                None
            }
            Way::Walked => {
                let mut walker = Walker::new(Arc::downgrade(&watcher));
                walker.keep_up(self, &places)?;
                Some(walker)
            }
        };
        std::thread::Builder::new()
            .name("furca-watch".to_owned())
            .spawn(move || gather(&heard, &places, walker, on_batch))
            .map_err(|e| Error::Watch(e.into()))?;
        Ok(Watcher { _notify: watcher })
    }
}

fn poisoned() -> Error {
    Error::Watch("the file system watcher is poisoned".into())
}

/// The folders of a git directory that can say anything `git_place` listens
/// for, besides the directory itself: not its thousands of object folders.
const GIT_FOLDERS: [&str; 4] = ["refs", "info", "rebase-merge", "rebase-apply"];

/// The folders worth a watch of their own: every folder of the working tree but
/// what git ignores and does not track, and of the git directories what can say
/// anything. A symbolic link is not followed, as git does not follow one.
fn folders_to_watch(repo: &Repository, places: &Places) -> Result<BTreeSet<PathBuf>, Error> {
    let mut folders = BTreeSet::new();
    for dir in [&places.git_dir, &places.common_dir] {
        folders.insert(dir.clone());
        for name in GIT_FOLDERS {
            let mut below = vec![dir.join(name)];
            while let Some(folder) = below.pop() {
                let Ok(entries) = std::fs::read_dir(&folder) else {
                    continue;
                };
                folders.insert(folder);
                below.extend(
                    entries
                        .flatten()
                        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                        .map(|entry| entry.path()),
                );
            }
        }
    }
    let Some(workdir) = &places.workdir else {
        return Ok(folders);
    };

    let index = repo.inner.index_or_empty().map_err(wrap(Error::Index))?;
    let mut excludes = repo
        .inner
        .excludes(
            &index,
            None,
            gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
        )
        .map_err(wrap(Error::Status))?;
    let mut below = vec![(workdir.clone(), String::new())];
    while let Some((folder, relative)) = below.pop() {
        // A folder that has gone since it was listed, or cannot be read, has
        // nothing to hear.
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        folders.insert(folder);
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            // A name that is not UTF-8 cannot be asked about, and is watched:
            // `working_tree_place` reads everything when it hears from one.
            let Some(name) = name.to_str() else {
                below.push((entry.path(), relative.clone()));
                continue;
            };
            if name.eq_ignore_ascii_case(".git") {
                continue;
            }
            let path = if relative.is_empty() {
                name.to_owned()
            } else {
                format!("{relative}/{name}")
            };
            // Git ignores nothing it tracks: a file added with `--force` under
            // an ignored folder is as much the working copy as any other.
            let tracked = index
                .prefixed_entries(format!("{path}/").as_bytes().as_bstr())
                .is_some_and(|entries| !entries.is_empty());
            let ignored = !tracked
                && excludes
                    .at_entry(
                        path.as_bytes().as_bstr(),
                        Some(gix::index::entry::Mode::DIR),
                    )
                    .map_err(wrap(Error::Status))?
                    .is_excluded();
            if !ignored {
                below.push((entry.path(), path));
            }
        }
    }
    Ok(folders)
}

/// Keeps a watch on each folder worth one, as folders come and go and as what git
/// ignores changes.
struct Walker {
    // Weak: only the `Watcher` keeps the file system's watcher alive, and it is
    // dropped to stop this thread.
    notify: Weak<Mutex<RecommendedWatcher>>,
    watching: BTreeSet<PathBuf>,
    /// Something has moved since the folders were last walked.
    behind: bool,
}

impl Walker {
    fn new(notify: Weak<Mutex<RecommendedWatcher>>) -> Self {
        Self {
            notify,
            watching: BTreeSet::new(),
            behind: false,
        }
    }

    /// Notes what an event means for the folders. A folder that moved or went is
    /// let go at once: its watch follows the folder, not the name, and one of the
    /// same name made in its place would otherwise go unheard.
    fn hear(&mut self, event: &notify::Event) {
        let renamed = matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)));
        let left = renamed || matches!(event.kind, EventKind::Remove(_));
        let arrived = renamed || matches!(event.kind, EventKind::Create(_));
        for path in &event.paths {
            if left && self.let_go(path) {
                self.behind = true;
            }
            if path.file_name().is_some_and(|name| name == ".gitignore")
                || (arrived && path.is_dir())
            {
                self.behind = true;
            }
        }
    }

    /// Stops watching `path` and what is under it; whether it was watched.
    fn let_go(&mut self, path: &Path) -> bool {
        let under: Vec<PathBuf> = self
            .watching
            .range(path.to_path_buf()..)
            .take_while(|folder| folder.starts_with(path))
            .cloned()
            .collect();
        if under.is_empty() {
            return false;
        }
        if let Some(notify) = self.notify.upgrade()
            && let Ok(mut notify) = notify.lock()
        {
            for folder in &under {
                // One that has gone took its watch with it.
                let _ = notify.unwatch(folder);
            }
        }
        for folder in &under {
            self.watching.remove(folder);
        }
        true
    }

    /// Walks the folders and watches what is new among them. Before a batch goes
    /// out, which is what closes the gap: what was written in a new folder before
    /// its watch is there is read with the batch that names the folder, and what
    /// is written after is heard.
    fn keep_up(&mut self, repo: &Repository, places: &Places) -> Result<(), Error> {
        self.behind = false;
        let wanted = folders_to_watch(repo, places)?;
        let Some(notify) = self.notify.upgrade() else {
            return Ok(());
        };
        let mut notify = notify.lock().map_err(|_| poisoned())?;
        for gone in self.watching.difference(&wanted) {
            let _ = notify.unwatch(gone);
        }
        self.watching.retain(|folder| wanted.contains(folder));
        for folder in wanted {
            if self.watching.contains(&folder) {
                continue;
            }
            match notify.watch(&folder, RecursiveMode::NonRecursive) {
                Ok(()) => {
                    self.watching.insert(folder);
                }
                // It went between the walk and the watch.
                Err(error) if matches!(error.kind, notify::ErrorKind::PathNotFound) => {}
                Err(error) => return Err(Error::Watch(error.into())),
            }
        }
        Ok(())
    }

    /// Catches up before a batch goes out, opening the repository as it now is;
    /// why it could not, for the batch to say.
    fn before_a_batch(&mut self, places: &Places) -> Option<String> {
        if !self.behind {
            return None;
        }
        Repository::at(&places.opened)
            .and_then(|repo| self.keep_up(&repo, places))
            .err()
            .map(|error| error.to_string())
    }
}

/// Where a repository's files are, in the form the file system reports them.
struct Places {
    /// Where the repository opens from, as it was found.
    opened: PathBuf,
    workdir: Option<PathBuf>,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

impl Places {
    fn of(repo: &Repository) -> Result<Self, Error> {
        let real = |path: &Path| std::fs::canonicalize(path).map_err(|e| Error::Watch(Box::new(e)));
        Ok(Self {
            opened: repo.workdir().unwrap_or(repo.git_dir()).to_path_buf(),
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
    unwatched: Option<String>,
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
            unwatched: taken.unwatched,
        }
    }
}

/// Gathers events into batches until the watcher is dropped.
fn gather(
    heard: &Receiver<notify::Result<notify::Event>>,
    places: &Places,
    mut walker: Option<Walker>,
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
                if let Some(walker) = walker.as_mut() {
                    gathering.unwatched = walker.before_a_batch(places);
                }
                let batch = gathering.take();
                if !batch.is_empty() {
                    on_batch(batch);
                }
            }
            Ok(event) => {
                let now = Instant::now();
                let lost = |gathering: &mut Gathering, walker: &mut Option<Walker>| {
                    gathering.add(Place::Everything, now);
                    if let Some(walker) = walker.as_mut() {
                        walker.behind = true;
                    }
                };
                match event {
                    // What the file system could not report it reports having
                    // lost: every event since is only a guess, and a folder
                    // made among them goes unheard until it is walked.
                    Err(_) => lost(&mut gathering, &mut walker),
                    Ok(event) if event.need_rescan() => lost(&mut gathering, &mut walker),
                    // A read is not a change, and our own reads would never end.
                    Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
                    Ok(event) => {
                        for path in &event.paths {
                            if !only_touches_a_folder(&event.kind, path) {
                                let place = places.place(path);
                                // What git ignores may have changed with it.
                                if place == Place::Everything
                                    && let Some(walker) = walker.as_mut()
                                {
                                    walker.behind = true;
                                }
                                gathering.add(place, now);
                            }
                        }
                        if let Some(walker) = walker.as_mut() {
                            walker.hear(&event);
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

    /// A repository whose `.gitignore` says `target/` and `node_modules/`, with
    /// `vendor/` ignored and one file under it added anyway.
    fn sample() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = std::fs::canonicalize(dir.path()).expect("real path");
        let git = |args: &[&str]| {
            let done = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .expect("git runs");
            assert!(done.status.success(), "{args:?}: {done:?}");
        };
        git(&["init", "--quiet"]);
        for folder in [
            "src/deep",
            "target/debug",
            "node_modules/pack/lib",
            "vendor/kept/inner",
            "vendor/other",
        ] {
            std::fs::create_dir_all(root.join(folder)).expect("folder");
        }
        let write = |path: &str, text: &str| std::fs::write(root.join(path), text).expect("write");
        write(".gitignore", "target/\nnode_modules/\nvendor/\n");
        write("src/deep/a.txt", "a");
        write("vendor/kept/file.txt", "kept");
        git(&["add", "-f", "vendor/kept/file.txt"]);
        (dir, root)
    }

    fn folders(root: &Path) -> BTreeSet<String> {
        let repo = Repository::open(root).expect("opens");
        let places = Places::of(&repo).expect("places");
        folders_to_watch(&repo, &places)
            .expect("walks")
            .iter()
            .map(|folder| {
                let inside = folder.strip_prefix(root).expect("under the root");
                inside.to_string_lossy().replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn the_walk_leaves_out_what_git_ignores_and_keeps_what_it_tracks() {
        let (_dir, root) = sample();
        let walked = folders(&root);
        let working: BTreeSet<&str> = walked
            .iter()
            .map(String::as_str)
            .filter(|folder| !folder.starts_with(".git"))
            .collect();
        assert_eq!(
            working,
            BTreeSet::from(["", "src", "src/deep", "vendor", "vendor/kept"]),
            "an ignored folder with nothing tracked under it is left out"
        );
    }

    #[test]
    fn the_walk_of_a_git_directory_takes_what_can_say_anything() {
        let (_dir, root) = sample();
        std::fs::create_dir_all(root.join(".git/refs/heads/feature")).expect("folder");
        std::fs::create_dir_all(root.join(".git/rebase-merge")).expect("folder");
        std::fs::create_dir_all(root.join(".git/lfs/objects")).expect("folder");
        let walked = folders(&root);
        for folder in [
            ".git",
            ".git/refs",
            ".git/refs/heads",
            ".git/refs/heads/feature",
            ".git/rebase-merge",
        ] {
            assert!(walked.contains(folder), "{folder} is watched: {walked:?}");
        }
        for folder in [".git/objects", ".git/hooks", ".git/lfs", ".git/lfs/objects"] {
            assert!(!walked.contains(folder), "{folder} is not: {walked:?}");
        }
    }

    #[test]
    fn a_folder_goes_from_the_walk_with_the_rule_that_ignores_it() {
        let (_dir, root) = sample();
        assert!(!folders(&root).contains("target"));
        std::fs::write(root.join(".gitignore"), "node_modules/\nsrc/\n").expect("write");
        let walked = folders(&root);
        assert!(walked.contains("target/debug"), "{walked:?}");
        assert!(!walked.contains("src"), "{walked:?}");
    }

    /// What is heard, up to the moment `path` is named; fails past a patience
    /// long enough for a loaded runner.
    fn hear_of(batches: &Receiver<Batch>, path: &str) -> Vec<String> {
        let until = Instant::now() + Duration::from_secs(20);
        let mut heard = Vec::new();
        while Instant::now() < until {
            if let Ok(batch) = batches.recv_timeout(Duration::from_millis(100)) {
                assert_eq!(batch.unwatched, None);
                let found = batch.paths.iter().any(|each| each == path);
                heard.extend(batch.paths);
                if found {
                    return heard;
                }
            }
        }
        panic!("{path} was never named: heard {heard:?}");
    }

    #[test]
    fn walked_a_new_folder_is_heard_and_an_ignored_one_is_not() {
        let (_dir, root) = sample();
        let repo = Repository::open(&root).expect("opens");
        let (tell, batches) = mpsc::channel();
        let watcher = repo
            .watch_the_way(Way::Walked, move |batch| {
                let _ = tell.send(batch);
            })
            .expect("watches");

        // A write in an ignored folder is not worth a watch, so the one after it
        // is the first thing heard of either.
        std::fs::write(root.join("target/debug/out.bin"), "built").expect("write");
        std::fs::write(root.join("src/deep/a.txt"), "changed").expect("write");
        let heard = hear_of(&batches, "src/deep/a.txt");
        assert!(
            !heard.iter().any(|path| path.starts_with("target/")),
            "{heard:?}"
        );

        // A folder made now: the watch on it is there by the time its batch is.
        std::fs::create_dir_all(root.join("src/new/inner")).expect("folder");
        hear_of(&batches, "src/new");
        std::fs::write(root.join("src/new/inner/b.txt"), "b").expect("write");
        hear_of(&batches, "src/new/inner/b.txt");

        // A rule taken away: what it hid is heard from then on.
        std::fs::write(root.join(".gitignore"), "node_modules/\nvendor/\n").expect("write");
        hear_of(&batches, ".gitignore");
        std::fs::write(root.join("target/debug/out.bin"), "again").expect("write");
        hear_of(&batches, "target/debug/out.bin");

        // A folder that went and came back under its own name is heard again.
        std::fs::remove_dir_all(root.join("src/new")).expect("gone");
        hear_of(&batches, "src/new");
        std::fs::create_dir_all(root.join("src/new")).expect("back");
        hear_of(&batches, "src/new");
        std::fs::write(root.join("src/new/c.txt"), "c").expect("write");
        hear_of(&batches, "src/new/c.txt");

        drop(watcher);
        let until = Instant::now() + Duration::from_secs(20);
        loop {
            match batches.recv_timeout(Duration::from_millis(100)) {
                Err(RecvTimeoutError::Disconnected) => break,
                _ => assert!(Instant::now() < until, "the thread outlived its watcher"),
            }
        }
    }

    /// A rename keeps the inode's watch under the old name: Windows will not move
    /// a folder a handle is held on at all, so this is Linux's to prove.
    #[cfg(target_os = "linux")]
    #[test]
    fn walked_a_folder_moved_is_heard_under_its_new_name() {
        let (_dir, root) = sample();
        let repo = Repository::open(&root).expect("opens");
        let (tell, batches) = mpsc::channel();
        let _watcher = repo
            .watch_the_way(Way::Walked, move |batch| {
                let _ = tell.send(batch);
            })
            .expect("watches");
        std::fs::rename(root.join("src/deep"), root.join("src/moved")).expect("moved");
        hear_of(&batches, "src/moved");
        std::fs::write(root.join("src/moved/a.txt"), "changed").expect("write");
        hear_of(&batches, "src/moved/a.txt");
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
