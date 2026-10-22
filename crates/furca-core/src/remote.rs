use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};

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
