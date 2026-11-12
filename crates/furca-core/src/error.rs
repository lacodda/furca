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
    #[error("could not read what {commit} changed: {source}")]
    Changes {
        commit: String,
        #[source]
        source: Source,
    },
    #[error("could not compare {path}: {source}")]
    Diff {
        path: String,
        #[source]
        source: Source,
    },
    #[error("could not read the working tree's changes: {0}")]
    Status(#[source] Source),
    #[error("could not read the submodules: {0}")]
    Submodules(#[source] Source),
    #[error("could not watch the working tree: {0}")]
    Watch(#[source] Source),
    #[error("could not write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
    /// A change to `path` is not what was asked for any more: it was shown, and
    /// has moved on since.
    #[error("{path} changed since it was read; look at it again")]
    Moved { path: String },
    /// `path` changes as a whole - added, deleted, moved, not text - and has no
    /// hunk to take apart from the rest.
    #[error("{path} changes as a whole file, not by hunks")]
    NotByHunks { path: String },
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
    /// git could not get into a remote: it had no credential for the address,
    /// the server took none of ssh's keys, or ssh does not know the server's
    /// key. furca never asks for a password, so `hint` says where git looks.
    #[error("`{command}` could not get into the remote: {hint}.{}", if stderr.is_empty() { String::new() } else { format!("\n{stderr}") })]
    Access {
        command: String,
        /// What to do about it.
        hint: &'static str,
        /// What git printed on standard error.
        stderr: String,
    },
    /// What was asked of a remote has no answer in this repository as it
    /// stands: `HEAD` on no branch, a branch that follows nothing, a remote
    /// that is not there.
    #[error("{0}")]
    Remote(String),
    /// What was asked of a branch or a tag has no answer in this repository
    /// as it stands: a name that is not here, a branch `HEAD` is on, a name
    /// taken by a branch that follows something else.
    #[error("{0}")]
    Ref(String),
    /// Deleting `branch` would leave `commits` commits that nothing else
    /// names - no other branch, remote-tracking branch or tag, nor `HEAD`.
    #[error("{branch} holds {}", only_it_holds(*commits))]
    Unmerged { branch: String, commits: usize },
}

/// What a branch alone holds, as [`Error::Unmerged`] says it.
fn only_it_holds(commits: usize) -> String {
    if commits == 1 {
        "1 commit that no other branch or tag has: deleting it anyway loses it".to_owned()
    } else {
        format!("{commits} commits that no other branch or tag has: deleting it anyway loses them")
    }
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
