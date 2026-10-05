use std::path::PathBuf;

/// Why a release could not even be planned.
///
/// A plan that finds problems is not an error: it is a plan whose checks fail.
/// These are the cases where there is nothing to check against.
#[derive(Debug, thiserror::Error)]
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
    #[error(transparent)]
    Git(#[from] furca_core::Error),
}
