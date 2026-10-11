/// Errors returned by furca-core.
///
/// Each variant names the read or the git run that failed, so a message is
/// useful on its own
/// in a terminal or a window. gix's errors carry a lot of context and differ
/// from call to call - some are `std::error::Error` types, some are `Exn`
/// trees that only convert into one; either way they are kept as boxed
/// sources so that a `Result<T, Error>` stays cheap to move whatever the
/// variant.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("could not open the repository: {0}")]
    Open(#[source] Source),
    #[error("could not read HEAD: {0}")]
    Head(#[source] Source),
    #[error("could not read the references: {0}")]
    References(#[source] Source),
    #[error("could not walk the history: {0}")]
    Walk(#[source] Source),
    #[error("could not read the index: {0}")]
    Index(#[source] Source),
    #[error("could not resolve `{revision}` to a commit: {source}")]
    Revision {
        revision: String,
        #[source]
        source: Source,
    },
    #[error("could not read commit {id}: {source}")]
    Commit {
        id: String,
        #[source]
        source: Source,
    },
    #[error("could not read the working tree's changes: {0}")]
    Status(#[source] Source),
    #[error("could not run git: {source}; is it installed and on PATH?")]
    GitMissing {
        #[source]
        source: std::io::Error,
    },
    #[error("`{command}` failed{}{}", code.map(|c| format!(" (exit {c})")).unwrap_or_default(), if stderr.is_empty() { String::new() } else { format!(": {stderr}") })]
    Git {
        command: String,
        code: Option<i32>,
        /// What git printed on standard error.
        stderr: String,
    },
}

/// A boxed error from gix, whose concrete type is not part of this API.
pub type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Boxes any gix error into [`Source`] for the variant `wrap` picks: a plain
/// error type, or an `Exn`, which converts with its whole tree of causes.
pub(crate) fn wrap<E>(variant: fn(Source) -> Error) -> impl FnOnce(E) -> Error
where
    E: Into<Source>,
{
    move |error| variant(error.into())
}
