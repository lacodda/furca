use std::collections::BTreeSet;

use crate::Repository;
use crate::error::{Error, wrap};

impl Repository {
    /// Every tracked path whose content differs from `HEAD`, staged or not,
    /// relative to the root with `/` separators and sorted. Untracked files
    /// are left out: they are not part of any commit until someone adds
    /// them.
    ///
    /// A file whose timestamps changed while its content did not is not a
    /// change: the content is compared, as `git status` does.
    pub fn changed_paths(&self) -> Result<Vec<String>, Error> {
        let iter = self
            .inner
            .status(gix::progress::Discard)
            .map_err(wrap(Error::Status))?
            .untracked_files(gix::status::UntrackedFiles::None)
            .index_worktree_rewrites(None)
            .index_worktree_submodules(gix::status::Submodule::AsConfigured { check_dirty: true })
            .into_iter(Vec::new())
            .map_err(wrap(Error::Status))?;
        let mut paths = BTreeSet::new();
        for item in iter {
            let item = item.map_err(wrap(Error::Status))?;
            paths.insert(item.location().to_string());
        }
        Ok(paths.into_iter().collect())
    }
}
