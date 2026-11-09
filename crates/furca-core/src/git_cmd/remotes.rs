//! What talks to a remote, and the remotes themselves: each a run of the
//! system `git`, so its credential helper, ssh-agent, hooks and configuration
//! apply as in a terminal (ADR 0021).
//!
//! What a fetch or a prune moved is read from the refs before and after it,
//! in-process, rather than from what git printed: git's words follow its
//! language and its version, and the refs are the fact.
//!
//! A [`Repository`](crate::Repository) reads the configuration once, when it
//! is opened: after a remote is added, renamed, pointed elsewhere or removed,
//! open it again to read the remotes as they are.

use super::{Git, Hear};
use crate::error::Error;
use crate::remote::{Destination, Fetched, Pulled, Pushed, Reconcile, Remotes, moved};

impl Git<'_> {
    /// Fetches from the remotes `from` names - every one of them with
    /// [`Remotes::All`], as `git fetch --all` does - telling `hear` how far
    /// it has got. `prune` also takes away the remote-tracking branches whose
    /// branch the remote no longer has; without it `fetch.prune` decides.
    pub fn fetch(&self, from: &Remotes, prune: bool, hear: Hear<'_>) -> Result<Fetched, Error> {
        let remotes = self.repo.fetch_remotes(from)?;
        let before = self.repo.fetched_refs()?;
        let mut args = vec!["fetch", "--progress"];
        if prune {
            args.push("--prune");
        }
        match from {
            Remotes::All => args.push("--all"),
            Remotes::Default | Remotes::Named(_) => args.push(&remotes[0]),
        }
        self.reach(&args, Some(hear))?;
        let after = self.repo.fetched_refs()?;
        Ok(Fetched {
            remotes,
            moved: moved(&before, &after),
        })
    }

    /// Pulls into the current branch from the branch it follows, putting the
    /// two together the way `how` says, and tells `hear` how far it has got.
    /// A merge takes git's own message. One that meets a conflict stops with
    /// git's words, and leaves the conflict to resolve as git leaves it.
    pub fn pull(&self, how: Reconcile, hear: Hear<'_>) -> Result<Pulled, Error> {
        let head = self.repo.head()?;
        let Some(branch) = head.branch else {
            return Err(Error::Remote(
                "HEAD is on no branch: there is nothing to pull into".to_owned(),
            ));
        };
        let Some(upstream) = self.repo.upstream()? else {
            return Err(Error::Remote(format!(
                "{branch} follows no remote branch: push it once and it will, or pull a branch by name with git"
            )));
        };
        let mut args = vec!["pull", "--progress"];
        args.extend(match how {
            Reconcile::AsConfigured => None,
            Reconcile::Merge => Some("--no-rebase"),
            Reconcile::Rebase => Some("--rebase"),
            Reconcile::FastForward => Some("--ff-only"),
        });
        self.reach(&args, Some(hear))?;
        Ok(Pulled {
            upstream: upstream.tracking.clone().unwrap_or_else(|| {
                let there = upstream.merge.trim_start_matches("refs/heads/");
                format!("{}/{there}", upstream.remote)
            }),
            branch,
            from: head.commit,
            to: self.repo.head()?.commit,
        })
    }

    /// Pushes the branch `to` describes, telling `hear` how far it has got.
    ///
    /// The annotated tags on the history it pushes go with it, so a release
    /// tag reaches every remote the branch does; a plain tag is a bookmark,
    /// and stays. With `force` it replaces what the remote's branch holds, but
    /// only if that is still what the remote-tracking branch says - a lease:
    /// what someone pushed since the last fetch makes git refuse rather than
    /// lose it.
    pub fn push_branch(
        &self,
        to: &Destination,
        force: bool,
        hear: Hear<'_>,
    ) -> Result<Pushed, Error> {
        let source = format!("refs/heads/{}", to.branch);
        let commit = self.repo.ref_target(&source)?.ok_or_else(|| {
            Error::Remote(format!(
                "{} has no commit yet: there is nothing to push",
                to.branch
            ))
        })?;
        let mut args = vec!["push", "--progress", "--follow-tags"];
        if to.set_upstream {
            args.push("--set-upstream");
        }
        let lease;
        if force {
            // What the remote's branch is expected to hold, named outright:
            // a fetch in the background between this read and the push moves
            // the remote-tracking branch, and an implicit lease would follow
            // it. None at all expects the remote to have no such branch.
            let expected = self.repo.ref_target(&to.tracking)?.unwrap_or_default();
            lease = format!("--force-with-lease={}:{expected}", to.to);
            args.push(&lease);
        }
        let refspec = format!("{source}:{}", to.to);
        args.extend([to.remote.as_str(), refspec.as_str()]);
        self.reach(&args, Some(hear))?;
        Ok(Pushed {
            remote: to.remote.clone(),
            branch: to.branch.clone(),
            to: to.to.clone(),
            commit,
            forced: force,
            upstream_set: to.set_upstream,
        })
    }

    /// Adds the remote `name`, fetched from `url`. Nothing is fetched yet.
    pub fn remote_add(&self, name: &str, url: &str) -> Result<(), Error> {
        self.run(&["remote", "add", "--", name, url]).map(drop)
    }

    /// Renames the remote `from` to `to`: its remote-tracking branches move
    /// with it, and the branches that follow it follow it under its new name.
    pub fn remote_rename(&self, from: &str, to: &str) -> Result<(), Error> {
        self.run(&["remote", "rename", "--", from, to]).map(drop)
    }

    /// Points the remote `name` at `url`, for fetching and - unless a
    /// `pushurl` sends them elsewhere - for pushing.
    pub fn remote_set_url(&self, name: &str, url: &str) -> Result<(), Error> {
        self.run(&["remote", "set-url", "--", name, url]).map(drop)
    }

    /// Removes the remote `name`: its configuration and its remote-tracking
    /// branches go, and a branch that followed it follows nothing. What is on
    /// the remote is not touched.
    pub fn remote_remove(&self, name: &str) -> Result<(), Error> {
        self.run(&["remote", "remove", "--", name]).map(drop)
    }

    /// Takes away the remote-tracking branches of `name` whose branch the
    /// remote no longer has, asking the remote which those are. The ones it
    /// took away.
    pub fn remote_prune(&self, name: &str) -> Result<Fetched, Error> {
        let before = self.repo.fetched_refs()?;
        self.reach(&["remote", "prune", "--", name], None)?;
        let after = self.repo.fetched_refs()?;
        Ok(Fetched {
            remotes: vec![name.to_owned()],
            moved: moved(&before, &after),
        })
    }
}
