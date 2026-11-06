//! The working copy of the tab in front, kept up to date while the tab is.
//!
//! One thread holds the status (`StatusCache`) and the watcher, and hears what
//! changes; the window hears a `working-copy` event each time the list moved,
//! and `refs-moved` when `HEAD` or a ref did, for the graph and the left column
//! to read themselves again. Only the tab in front is watched: a tab behind it
//! shows nothing that would need keeping, and the line is eighteen repositories
//! of which any one may be in the middle of a build.
//!
//! The list goes out as soon as it is read and fills in as its files are:
//! what each holds, and how many lines moved, a slice at a time between hearing
//! what else changed. A list too long for anyone to read is cut for the window,
//! which says how many more there are; `furca status` prints them all.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use furca_core::{Batch, FileChange, Refreshed, Repository, Status, StatusCache, Watcher};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

/// The event the list goes out on.
pub const EVENT: &str = "working-copy";
/// The event that says refs moved.
pub const REFS_MOVED: &str = "refs-moved";

/// Rows of each list the window is given.
const ROWS: usize = 1_000;
/// Files read in one go, between hearing what else changed.
const SLICE: usize = 32;
/// How long the first reading may hold the list back: a save reads one file,
/// and a row that arrives whole does not flicker.
const FIRST_READING: Duration = Duration::from_millis(30);
/// How often the list goes out again while a long one is read.
const READING_EVERY: Duration = Duration::from_millis(250);
/// Files read after any one change: as many rows as the window can be given.
const MOST_READ: usize = 3 * ROWS;

/// What the window hears of a repository's working copy.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct WorkingCopy {
    /// The repository, as its tab knows it.
    pub path: String,
    pub read: WorkingRead,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum WorkingRead {
    Read {
        /// The lists, each cut to the rows the window is given.
        status: Status,
        /// How long the lists really are.
        counts: Counts,
        /// What moved since the last time.
        refreshed: Refreshed,
        /// Why nothing is watching the working tree, if nothing is: the list
        /// is then only as fresh as the last time the window got the focus.
        unwatched: Option<String>,
    },
    Failed {
        reason: String,
    },
}

/// How long each list really is.
#[derive(Debug, Clone, Copy, Serialize, ts_rs::TS)]
pub struct Counts {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
}

/// `HEAD` or a ref moved in the repository at `path`.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct RefsMoved {
    pub path: String,
}

enum Word {
    Heard(Batch),
    ReadAgain,
    Stop,
}

struct Front {
    path: String,
    tell: Sender<Word>,
}

/// The one repository being watched.
#[derive(Default)]
pub struct Watched(Mutex<Option<Front>>);

/// Watches the repository at `path` instead of the one watched now; `None`
/// when no tab is in front. Asked again for the one it is watching, it reads
/// the whole status again: the window got the focus back, and what happened
/// while it did not is worth a look however well the watcher listened.
#[tauri::command]
pub async fn watch_working_copy(
    app: AppHandle,
    watched: State<'_, Watched>,
    path: Option<String>,
) -> Result<(), String> {
    let mut front = watched
        .0
        .lock()
        .map_err(|_| "the watch on the working copy is poisoned")?;
    if let (Some(path), Some(now)) = (&path, front.as_ref())
        && &now.path == path
    {
        let _ = now.tell.send(Word::ReadAgain);
        return Ok(());
    }
    if let Some(old) = front.take() {
        let _ = old.tell.send(Word::Stop);
    }
    if let Some(path) = path {
        let (tell, told) = mpsc::channel();
        let keeper = Keeper {
            app,
            path: path.clone(),
            tell: tell.clone(),
            cache: None,
            watcher: None,
            unwatched: None,
        };
        std::thread::Builder::new()
            .name("furca-working-copy".to_owned())
            .spawn(move || keeper.run(&told))
            .map_err(|e| format!("could not start watching: {e}"))?;
        *front = Some(Front { path, tell });
    }
    Ok(())
}

/// The thread that holds a repository's status.
struct Keeper {
    app: AppHandle,
    path: String,
    /// For the watcher to speak on.
    tell: Sender<Word>,
    cache: Option<StatusCache>,
    watcher: Option<Watcher>,
    unwatched: Option<String>,
}

impl Keeper {
    fn run(mut self, told: &Receiver<Word>) {
        self.take(Word::ReadAgain);
        // How many files were read since the last change, and when the list
        // last went out while they were.
        let mut read = 0;
        let mut sent = Instant::now();
        loop {
            let reading =
                read < MOST_READ && self.cache.as_ref().is_some_and(StatusCache::has_unread);
            let word = if reading {
                match told.try_recv() {
                    Ok(word) => Some(word),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match told.recv() {
                    Ok(word) => Some(word),
                    Err(_) => return,
                }
            };
            match word {
                Some(Word::Stop) => return,
                Some(word) => {
                    self.take(word);
                    read = 0;
                    sent = Instant::now();
                }
                None => {
                    read += self.read_slice(READING_EVERY.saturating_sub(sent.elapsed()));
                    if sent.elapsed() >= READING_EVERY || !self.has_more() {
                        self.send(Refreshed::Nothing);
                        sent = Instant::now();
                    }
                }
            }
        }
    }

    fn has_more(&self) -> bool {
        self.cache.as_ref().is_some_and(StatusCache::has_unread)
    }

    /// Brings the list up to date for `word`, reads what is quick to read and
    /// sends the list.
    fn take(&mut self, word: Word) {
        // Opened for each change, so the configuration it reads is current and
        // the handle holds nothing of a repository that moved on.
        let repo = match Repository::at(&self.path) {
            Ok(repo) => repo,
            Err(error) => return self.fail(error.to_string()),
        };
        if self.watcher.is_none() && self.unwatched.is_none() {
            let tell = self.tell.clone();
            match repo.watch(move |batch| {
                let _ = tell.send(Word::Heard(batch));
            }) {
                Ok(watcher) => self.watcher = Some(watcher),
                Err(error) => self.unwatched = Some(error.to_string()),
            }
        }
        if let Word::Heard(Batch {
            unwatched: Some(why),
            ..
        }) = &word
        {
            self.unwatched = Some(why.clone());
        }
        let refreshed = match (self.cache.as_mut(), &word) {
            (Some(cache), Word::Heard(batch)) if !batch.everything => {
                cache.refresh(&repo, &batch.paths)
            }
            (Some(cache), _) => cache.read_again(&repo).map(|()| Refreshed::Everything),
            (None, _) => StatusCache::read(&repo).map(|cache| {
                self.cache = Some(cache);
                Refreshed::Everything
            }),
        };
        let refreshed = match refreshed {
            Ok(refreshed) => refreshed,
            Err(error) => return self.fail(error.to_string()),
        };
        if let Word::Heard(batch) = &word
            && (batch.refs || batch.everything)
        {
            let _ = self.app.emit(
                REFS_MOVED,
                RefsMoved {
                    path: self.path.clone(),
                },
            );
        }
        if refreshed != Refreshed::Nothing {
            self.read_slice(FIRST_READING);
            self.send(refreshed);
        }
    }

    /// Reads files for about `long`; returns how many.
    fn read_slice(&mut self, long: Duration) -> usize {
        let (Some(cache), Ok(repo)) = (self.cache.as_mut(), Repository::at(&self.path)) else {
            return MOST_READ;
        };
        let start = Instant::now();
        let mut read = 0;
        loop {
            let done = cache.read_files(&repo, SLICE);
            read += done;
            if done == 0 || start.elapsed() >= long {
                return read;
            }
        }
    }

    fn send(&self, refreshed: Refreshed) {
        let Some(cache) = &self.cache else { return };
        let status = cache.status();
        let cut = |files: &[FileChange]| files.iter().take(ROWS).cloned().collect();
        let read = WorkingRead::Read {
            status: Status {
                staged: cut(&status.staged),
                unstaged: cut(&status.unstaged),
                untracked: cut(&status.untracked),
                conflicted: status.conflicted.clone(),
            },
            counts: Counts {
                staged: status.staged.len(),
                unstaged: status.unstaged.len(),
                untracked: status.untracked.len(),
            },
            refreshed,
            unwatched: self.unwatched.clone(),
        };
        self.emit(read);
    }

    fn fail(&mut self, reason: String) {
        // The next word reads everything: what was held is not worth trusting.
        self.cache = None;
        self.emit(WorkingRead::Failed { reason });
    }

    fn emit(&self, read: WorkingRead) {
        let _ = self.app.emit(
            EVENT,
            WorkingCopy {
                path: self.path.clone(),
                read,
            },
        );
    }
}
