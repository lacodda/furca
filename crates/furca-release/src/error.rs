use std::path::PathBuf;

/// Why a release could not be planned or run at all.
///
/// A plan that finds problems is not an error: it is a plan whose checks
/// fail, and a run that stops on a red step is a run that says where it
/// stopped. These are the cases where there is nothing to check against, or
/// something the engine relies on did not work.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("no release.toml in {}", .0.display())]
    NoConfig(PathBuf),
    #[error("release.toml: {}", .0.join("; "))]
    Config(Vec<String>),
    #[error("{} is not a working tree: a bare repository has nothing to release", .0.display())]
    Bare(PathBuf),
    #[error("could not read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write the changelog section: {0}")]
    Changelog(String),
    /// A file the bump was to rewrite no longer holds what the plan saw.
    #[error("the bump left `{path}` alone: it {why}")]
    Bump { path: String, why: String },
    #[error(transparent)]
    Git(#[from] furca_core::Error),
    #[error("could not run the gate command `{command}`: {source}")]
    Gate {
        command: String,
        #[source]
        source: std::io::Error,
    },
    /// A forge or a registry could not be asked.
    #[error("{0}")]
    Forge(String),
    #[error("the release checkpoint {} cannot be read: {why}; delete it to start over", path.display())]
    Checkpoint { path: PathBuf, why: String },
}
