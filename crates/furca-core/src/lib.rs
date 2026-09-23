//! furca-core: a plain library over gitoxide, with no Tauri or terminal
//! knowledge of its own. See `docs/adr/0002-one-core-three-doors.md`.
//!
//! Everything here reads. Mutations go through the system `git` CLI and are
//! not part of this crate yet (ADR 0001).
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
mod log;
mod refs;
mod repo;

pub use error::Error;
pub use log::{Commit, Log, Person, Tips};
pub use refs::{Branch, Refs, RemoteBranch, Tag};
pub use repo::{HeadSummary, Repository};
