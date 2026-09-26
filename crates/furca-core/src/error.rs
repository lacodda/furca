/// Errors returned by furca-core.
///
/// Each variant names the read that failed, so a message is useful on its own
/// in a terminal or a window. gix's errors carry a lot of context and differ
/// from call to call - some are `std::error::Error` types, some are `Exn`
/// trees that only convert into one; either way they are kept as boxed
/// sources so that a `Result<T, Error>` stays cheap to move whatever the
/// variant.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not open the repository: {0}")]
    Open(#[source] Source),
    #[error("could not read HEAD: {0}")]
    Head(#[source] Source),
    #[error("could not read the references: {0}")]
    References(#[source] Source),
    #[error("could not walk the history: {0}")]
    Walk(#[source] Source),
    #[error("could not read commit {id}: {source}")]
    Commit {
        id: String,
        #[source]
        source: Source,
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
