use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};
use crate::log::moment;

/// One entry of the stash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Stash {
    /// Its place in the stash, newest first: `0` is `stash@{0}`.
    pub index: usize,
    /// What git wrote when it was made: `WIP on main: 1a2b3c4 subject` for
    /// a plain `git stash`, `On main: message` for one given a message.
    pub message: String,
    /// The stash commit.
    pub commit: String,
    /// When it was made, RFC 3339 with the maker's offset.
    pub time: String,
    /// The same moment as seconds since the Unix epoch.
    pub seconds: i64,
}

impl Repository {
    /// The stash, newest first, as `git stash list` shows it.
    ///
    /// The stash is the reflog of `refs/stash`: each `git stash` adds a
    /// line, each `drop` takes one out. No stash at all is an empty list.
    pub fn stashes(&self) -> Result<Vec<Stash>, Error> {
        let Some(stash) = self
            .inner
            .try_find_reference("refs/stash")
            .map_err(wrap(Error::References))?
        else {
            return Ok(Vec::new());
        };
        let mut platform = stash.log_iter();
        let Some(lines) = platform.all().map_err(wrap(Error::References))? else {
            return Ok(Vec::new());
        };
        let mut entries = Vec::new();
        for line in lines {
            let line = line.map_err(wrap(Error::References))?;
            let time = line.signature.time().map_err(wrap(Error::References))?;
            let (time, seconds) = moment(time);
            entries.push((
                line.new_oid.to_string(),
                line.message.to_string(),
                time,
                seconds,
            ));
        }
        // The reflog is written oldest first; `stash@{0}` is its last line.
        Ok(entries
            .into_iter()
            .rev()
            .enumerate()
            .map(|(index, (commit, message, time, seconds))| Stash {
                index,
                message,
                commit,
                time,
                seconds,
            })
            .collect())
    }
}
