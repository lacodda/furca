use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Repository;
use crate::error::{Error, wrap};

/// Which remotes a fetch or a push goes to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(tag = "to", content = "name", rename_all = "kebab-case")]
pub enum Remotes {
    /// The one git would pick. A fetch goes to the remote the current branch
    /// follows, or `origin`; a push goes where the branch is pushed - its
    /// `pushRemote`, `remote.pushDefault`, the remote it follows - or to the
    /// only remote there is, or `origin`.
    Default,
    /// The remote of this name.
    Named(String),
    /// Every remote, one after another.
    All,
}

/// A ref a fetch or a prune made, moved or took away: a remote-tracking
/// branch, or a tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Moved {
    /// The full name: `refs/remotes/origin/main`, `refs/tags/v1.0.0`.
    pub name: String,
    /// What it named before; `None` for one that is new.
    pub from: Option<String>,
    /// What it names now; `None` for one that is gone.
    pub to: Option<String>,
}

/// What a fetch did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Fetched {
    /// The remotes asked, in the order they were.
    pub remotes: Vec<String>,
    /// The remote-tracking branches and tags that moved, by name.
    pub moved: Vec<Moved>,
}

/// How a pull puts the remote's commits together with the branch's own:
/// git's own choice, with its flags' names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub enum Reconcile {
    /// As the repository's configuration says - `pull.rebase`, `pull.ff` -
    /// and as git refuses when it says nothing and the two have parted.
    AsConfigured,
    /// A merge: `--no-rebase`.
    Merge,
    /// The branch's own commits replayed on the remote's: `--rebase`.
    Rebase,
    /// Only a fast-forward, and a refusal when the two have parted:
    /// `--ff-only`.
    FastForward,
}

/// What a pull did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pulled {
    /// The branch pulled into.
    pub branch: String,
    /// The remote-tracking branch it was pulled from, such as `origin/main`.
    pub upstream: String,
    /// The commit `HEAD` was on before; `None` on a branch with no commit.
    pub from: Option<String>,
    /// The commit it is on now; the same as `from` when nothing came.
    pub to: Option<String>,
}

/// Where the current branch goes when it is pushed to one remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Destination {
    /// The local branch, such as `main`.
    pub branch: String,
    /// The remote's name.
    pub remote: String,
    /// The remote's address for pushing, without any password in it.
    pub url: Option<String>,
    /// The branch there, as a full name: the branch the local one follows on
    /// the remote it follows, and one of the same name anywhere else.
    pub to: String,
    /// The remote-tracking branch that mirrors it here, as a full name: what
    /// a push with a lease expects the remote to hold.
    pub tracking: String,
    /// The branch follows nothing yet, and will follow this one once pushed.
    pub set_upstream: bool,
}

/// What a push did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Pushed {
    pub remote: String,
    /// The local branch.
    pub branch: String,
    /// The branch on the remote, as a full name.
    pub to: String,
    /// The commit pushed.
    pub commit: String,
    /// It went with a lease: the remote's branch was replaced, if it held
    /// what the remote-tracking branch said it did.
    pub forced: bool,
    /// The branch follows the remote's one from now on.
    pub upstream_set: bool,
}

/// A configured remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Remote {
    /// Its name, such as `origin`.
    pub name: String,
    /// Where it is fetched from, with any password in the address left out.
    pub url: Option<String>,
    /// Where it is pushed to, when a `pushurl` sends the pushes elsewhere;
    /// `None` when they go where the fetches come from.
    pub push_url: Option<String>,
}

impl Repository {
    /// Every configured remote, sorted by name.
    ///
    /// The addresses are for showing: a password written into one
    /// (`https://name:token@host/...`) is a secret, and is dropped. The user
    /// name stays - it says which account the remote is reached as.
    pub fn remotes(&self) -> Result<Vec<Remote>, Error> {
        let mut remotes = Vec::new();
        for name in self.inner.remote_names() {
            let remote = self
                .inner
                .find_remote(&**name)
                .map_err(wrap(Error::References))?;
            let fetch = remote.url(gix::remote::Direction::Fetch).map(shown);
            let push = remote.url(gix::remote::Direction::Push).map(shown);
            remotes.push(Remote {
                name: name.to_string(),
                push_url: push.filter(|push| Some(push) != fetch.as_ref()),
                url: fetch,
            });
        }
        remotes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(remotes)
    }

    /// The names of the configured remotes, sorted.
    pub fn remote_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .inner
            .remote_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        names.sort();
        names
    }

    /// The remotes `to` names for a fetch: the one the current branch follows
    /// or `origin` - or the only remote there is - for [`Remotes::Default`].
    pub fn fetch_remotes(&self, to: &Remotes) -> Result<Vec<String>, Error> {
        let names = self.remote_names();
        if names.is_empty() {
            return Err(Error::Remote(
                "there is no remote to fetch from: add one first".to_owned(),
            ));
        }
        match to {
            Remotes::All => Ok(names),
            Remotes::Named(name) => Ok(vec![known(name, &names)?]),
            Remotes::Default => {
                let follows = self.upstream()?.map(|upstream| upstream.remote);
                follows
                    .filter(|name| names.contains(name))
                    .or_else(|| only_or_origin(&names))
                    .map(|name| vec![name])
                    .ok_or_else(|| {
                        Error::Remote(format!(
                            "the current branch follows no remote, and there are {}: name the one to fetch",
                            names.len()
                        ))
                    })
            }
        }
    }

    /// Where the current branch goes when it is pushed to the remotes `to`
    /// names, in the order it goes there.
    ///
    /// On the remote the branch follows it goes to the branch it follows
    /// there; anywhere else, to a branch of its own name. A branch that
    /// follows nothing is set to follow the one it is pushed to - the default
    /// remote's, when it goes to every remote.
    pub fn destinations(&self, to: &Remotes) -> Result<Vec<Destination>, Error> {
        let head = self.inner.head().map_err(wrap(Error::Head))?;
        let Some(full) = head.referent_name().map(ToOwned::to_owned) else {
            return Err(Error::Remote(
                "HEAD is on no branch: there is no branch to push".to_owned(),
            ));
        };
        let short = full.as_ref().shorten();
        let branch = short.to_string();
        let names = self.remote_names();
        if names.is_empty() {
            return Err(Error::Remote(
                "there is no remote to push to: add one first".to_owned(),
            ));
        }
        let upstream = self.upstream()?;
        // `pushRemote`, then `remote.pushDefault`, then the remote it follows.
        let pushed_to = self
            .inner
            .branch_remote_name(short, gix::remote::Direction::Push)
            .map(|name| name.as_bstr().to_string())
            .filter(|name| names.contains(name));
        let default = pushed_to.or_else(|| only_or_origin(&names));
        let chosen = match to {
            Remotes::All => names.clone(),
            Remotes::Named(name) => vec![known(name, &names)?],
            Remotes::Default => vec![default.clone().ok_or_else(|| {
                Error::Remote(format!(
                    "{branch} follows no remote, and there are {}: name the one to push to",
                    names.len()
                ))
            })?],
        };
        Ok(chosen
            .into_iter()
            .map(|remote| {
                let followed = upstream.as_ref().filter(|up| up.remote == remote);
                let to_ref =
                    followed.map_or_else(|| format!("refs/heads/{branch}"), |up| up.merge.clone());
                let tracking = followed
                    .and_then(|up| up.tracking.as_ref())
                    .map(|short| format!("refs/remotes/{short}"))
                    .unwrap_or_else(|| {
                        let name = to_ref.strip_prefix("refs/heads/").unwrap_or(&to_ref);
                        format!("refs/remotes/{remote}/{name}")
                    });
                let set_upstream = upstream.is_none()
                    && match to {
                        Remotes::All => default.as_ref() == Some(&remote),
                        Remotes::Default | Remotes::Named(_) => true,
                    };
                Destination {
                    branch: branch.clone(),
                    url: self.push_url(&remote),
                    to: to_ref,
                    tracking,
                    set_upstream,
                    remote,
                }
            })
            .collect())
    }

    /// The address `remote` is pushed to, without its password.
    fn push_url(&self, remote: &str) -> Option<String> {
        self.inner
            .try_find_remote(remote)
            .and_then(Result::ok)
            .and_then(|r| r.url(gix::remote::Direction::Push).map(shown))
    }

    /// The object every remote-tracking branch and tag names, by full name:
    /// what a fetch or a prune is measured against. A symbolic one -
    /// `origin/HEAD` - names another, and is left out.
    pub(crate) fn fetched_refs(&self) -> Result<BTreeMap<String, String>, Error> {
        let platform = self.inner.references().map_err(wrap(Error::References))?;
        let mut refs = BTreeMap::new();
        let remote = platform
            .remote_branches()
            .map_err(wrap(Error::References))?;
        let tags = platform.tags().map_err(wrap(Error::References))?;
        for reference in remote.chain(tags) {
            let reference = reference.map_err(wrap(Error::References))?;
            if let Some(id) = reference.target().try_id() {
                refs.insert(reference.name().as_bstr().to_string(), id.to_string());
            }
        }
        Ok(refs)
    }

    /// The object the ref `name` names, if there is such a ref.
    pub(crate) fn ref_target(&self, name: &str) -> Result<Option<String>, Error> {
        Ok(self
            .inner
            .try_find_reference(name)
            .map_err(wrap(Error::References))?
            .and_then(|mut reference| reference.peel_to_id().ok())
            .map(|id| id.to_string()))
    }
}

/// The refs that differ between `before` and `after`, by name.
pub(crate) fn moved(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<Moved> {
    let mut names: Vec<&String> = before.keys().chain(after.keys()).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|name| before.get(*name) != after.get(*name))
        .map(|name| Moved {
            name: name.clone(),
            from: before.get(name).cloned(),
            to: after.get(name).cloned(),
        })
        .collect()
}

/// `name`, when it is one of the remotes.
fn known(name: &str, names: &[String]) -> Result<String, Error> {
    if names.iter().any(|known| known == name) {
        Ok(name.to_owned())
    } else {
        Err(Error::Remote(format!(
            "there is no remote called `{name}`: the remotes are {}",
            names.join(", ")
        )))
    }
}

/// The only remote there is, or else `origin`, if there is one.
fn only_or_origin(names: &[String]) -> Option<String> {
    match names {
        [only] => Some(only.clone()),
        _ => names.iter().find(|name| *name == "origin").cloned(),
    }
}

/// An address as it may be shown: without its password.
pub(crate) fn shown(url: &gix::Url) -> String {
    let mut url = url.clone();
    url.set_password(None);
    url.to_bstring().to_string()
}

#[cfg(test)]
mod tests {
    use super::shown;

    #[test]
    fn a_password_in_an_address_is_never_shown() {
        let url = gix::Url::from_bytes("https://ada:s3cret@example.com/team/repo.git".into())
            .expect("a url");
        let text = shown(&url);
        assert!(!text.contains("s3cret"), "{text}");
        assert_eq!(text, "https://ada@example.com/team/repo.git");
    }

    #[test]
    fn an_address_without_one_is_shown_as_written() {
        for written in [
            "https://example.com/team/repo.git",
            "git@example.com:team/repo.git",
            "/srv/git/repo.git",
        ] {
            let url = gix::Url::from_bytes(written.into()).expect("a url");
            assert_eq!(shown(&url), written);
        }
    }
}
