//! furca-release: the release engine of the lacodda line, read from a
//! repository's `release.toml`.
//!
//! A release is the end of a stage, done as a machine of steps rather than by
//! hand: propose the next number, check that every shop window agrees, bump,
//! write the changelog, run the gate, commit, wait for CI, tag, publish, and
//! the steps after the tag. This crate plans it - what the next release is,
//! what stands in its way and what a run will do - and changes nothing.
//! Running the plan is the next release's work. See ADR 0006.
//!
//! ```no_run
//! use furca_release::{Release, Sources};
//!
//! let release = Release::open(".")?;
//! let plan = release.plan(&Sources {
//!     registries: Some(&furca_release::Http::default()),
//!     record: Some(&furca_release::Rigger),
//!     now: std::time::SystemTime::now(),
//! })?;
//! for check in &plan.checks {
//!     println!("{:?} {}: {}", check.status, check.name, check.summary);
//! }
//! # Ok::<(), furca_release::Error>(())
//! ```

mod commits;
mod config;
mod error;
mod manifest;
mod plan;
mod record;
mod registry;
mod texts;
mod version;

pub use commits::{Change, Tally};
pub use config::{Ci, Config, FILE, Gate, Package, Post, Project, Provider, Registry};
pub use error::Error;
pub use manifest::Field;
pub use plan::{
    Check, CiStep, Edit, Origin, Plan, Proposal, Publish, Release, Sources, Status, Steps,
};
pub use record::{Reading, Record, Rigger, Stage};
pub use registry::{Http, Registries};
pub use texts::Mention;
pub use version::Bump;
