//! furca-release: the release engine of the lacodda line, read from a
//! repository's `release.toml`.
//!
//! A release is the end of a stage, done as a machine of steps rather than by
//! hand: propose the next number, check that every shop window agrees, bump,
//! write the changelog, run the gate, commit, push, wait for CI, tag, wait
//! for the publish, check the shop windows, install for the owner and prove
//! it, deploy, and tell the owner's record. [`Release::plan`] says what the
//! next release is and what stands in its way, and changes nothing;
//! [`Release::run`] carries the plan out and stops at the first red step;
//! [`Release::resume`] carries a stopped run on; [`Release::verify`] reads
//! the shop windows of any release. See ADR 0006, 0007 and 0008.
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

mod bump;
mod changelog;
mod commits;
mod config;
mod error;
pub mod hosting;
mod install;
mod manifest;
mod plan;
mod record;
mod registry;
mod run;
mod texts;
mod verify;
mod version;
mod workflow;

pub use bump::Touched;
pub use commits::{Change, Tally};
pub use config::{
    Ci, Config, Deploy, FILE, Gate, Install, Package, PageAssets, Post, Project, Provider, Registry,
};
pub use error::Error;
pub use install::{INSTALLERS, Local, Machine, TARGET};
pub use manifest::Field;
pub use plan::{
    Check, CiStep, Edit, Origin, Plan, Proposal, Publish, Release, Sources, Status, Steps,
};
pub use record::{Kept, Reading, Record, Rigger, Shipped, Stage};
pub use registry::{Http, Registries};
pub use run::{CHECKPOINT, ForgeFor, Report, Runner, STEPS, Step, StepStatus};
pub use texts::Mention;
pub use verify::{Shops, Verified, Web};
pub use version::Bump;
