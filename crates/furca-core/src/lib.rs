//! furca-core: a plain library over gitoxide, with no Tauri or terminal
//! knowledge of its own. See `docs/adr/0002-one-core-three-doors.md`.
//!
//! Reads happen in-process through gitoxide. Mutations - commits, tags,
//! pushes - run the system `git` CLI, all of them through
//! [`Repository::git`], which logs every run (ADR 0001).
//!
//! ```no_run
//! let repo = furca_core::Repository::open(".")?;
//! let head = repo.head()?;
//! let log = repo.log(furca_core::Tips::Head, 20)?;
//! for commit in &log.commits {
//!     println!("{} {}", &commit.id[..7], commit.subject);
//! }
//! # Ok::<(), furca_core::Error>(())
//! ```

mod error;
mod git_cmd;
mod history;
mod log;
mod refs;
mod repo;
mod walk;
mod worktree;

pub use error::Error;
pub use git_cmd::{Git, GitRun};
pub use history::History;
pub use log::{Commit, Log, Person, Tips};
pub use refs::{Branch, Refs, RemoteBranch, Tag};
pub use repo::{HeadSummary, Repository, Upstream};
