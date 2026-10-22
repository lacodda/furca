use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};
use crate::remote::shown;

/// A submodule the repository declares in `.gitmodules`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Submodule {
    /// Its name in `.gitmodules`, which is often but not always its path.
    pub name: String,
    /// Where it lives in the working tree, with `/` separators.
    pub path: String,
    /// Where it is cloned from, with any password left out.
    pub url: Option<String>,
    /// The commit the repository records for it in the index: what a
    /// `git submodule update` would check out.
    pub recorded: Option<String>,
    /// The commit checked out in it; `None` when it is not checked out.
    pub checked_out: Option<String>,
}

impl Repository {
    /// The submodules, in the order `.gitmodules` lists them; none when the
    /// repository has no `.gitmodules`.
    ///
    /// A submodule whose path or address cannot be read is listed with what
    /// can be: one damaged entry is not a reason to hide the others.
    pub fn submodules(&self) -> Result<Vec<Submodule>, Error> {
        let Some(modules) = self.inner.submodules().map_err(wrap(Error::Submodules))? else {
            return Ok(Vec::new());
        };
        let mut list = Vec::new();
        for module in modules {
            let path = module
                .path()
                .map(|path| path.to_string())
                .unwrap_or_default();
            // The submodule's repository outlives its checkout: after a
            // `deinit` it still sits in `.git/modules` with a HEAD of its
            // own, so HEAD alone would call an empty directory checked out.
            let checked_out = match module.state() {
                Ok(state) if state.worktree_checkout => {
                    module.head_id().ok().flatten().map(|id| id.to_string())
                }
                _ => None,
            };
            list.push(Submodule {
                name: module.name().to_string(),
                url: module.url().ok().map(|url| shown(&url)),
                recorded: module.index_id().ok().flatten().map(|id| id.to_string()),
                checked_out,
                path,
            });
        }
        Ok(list)
    }
}
