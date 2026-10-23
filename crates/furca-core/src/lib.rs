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

mod commit_graph;
mod error;
mod find;
mod git_cmd;
mod graph;
mod history;
mod log;
mod refs;
mod remote;
mod repo;
mod standing;
mod stash;
mod submodule;
mod walk;
mod worktree;

pub use commit_graph::CommitGraph;
pub use error::Error;
pub use find::repositories;
pub use git_cmd::{Git, GitRun};
pub use graph::{COLORS, Graph, Query, Row, Segment, SegmentKind};
pub use history::History;
pub use log::{Commit, Log, Person, Tips};
pub use refs::{Branch, Refs, RemoteBranch, Tag};
pub use remote::Remote;
pub use repo::{HeadSummary, Repository, Upstream};
pub use standing::{NearestTag, Operation, Standing, Tracking};
pub use stash::Stash;
pub use submodule::Submodule;
