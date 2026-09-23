use serde::Serialize;

use crate::Repository;
use crate::error::{Error, wrap};

/// Every named pointer into the history: local branches, remote-tracking
/// branches and tags, each sorted by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Refs {
    pub branches: Vec<Branch>,
    pub remote_branches: Vec<RemoteBranch>,
    pub tags: Vec<Tag>,
}

/// A local branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Branch {
    /// The short name, such as `main` or `feature/login`.
    pub name: String,
    /// The commit the branch points at.
    pub target: String,
    /// The remote-tracking branch it follows, such as `origin/main`.
    pub upstream: Option<String>,
    /// Whether `HEAD` is on this branch.
    pub head: bool,
}

/// A remote-tracking branch, such as `origin/main`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteBranch {
    /// The short name including the remote, such as `origin/main`.
    pub name: String,
    /// The configured remote this branch belongs to, if one matches.
    pub remote: Option<String>,
    /// The commit the branch points at.
    pub target: String,
}

/// A tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// The short name, such as `v1.0.0`.
    pub name: String,
    /// The object the tag finally points at once annotated tags are peeled —
    /// almost always a commit.
    pub target: String,
    /// Whether this is an annotated tag object rather than a plain pointer.
    pub annotated: bool,
}

impl Repository {
    /// Lists the branches, remote-tracking branches and tags.
    ///
    /// Symbolic references among the remotes (`origin/HEAD`) are left out:
    /// they name another remote branch that is already in the list. A
    /// reference that cannot be peeled — one pointing at a missing object in a
    /// damaged or shallow clone — is left out rather than failing the list.
    pub fn refs(&self) -> Result<Refs, Error> {
        let platform = self.inner.references().map_err(wrap(Error::References))?;
        let head = self
            .inner
            .head_name()
            .map_err(wrap(Error::References))?
            .map(|name| name.shorten().to_string());

        let mut branches = Vec::new();
        for reference in platform.local_branches().map_err(wrap(Error::References))? {
            let mut reference = reference.map_err(Error::References)?;
            let Ok(target) = reference.peel_to_id() else {
                continue;
            };
            let name = reference.name().shorten().to_string();
            branches.push(Branch {
                upstream: self.upstream_of(reference.name()),
                head: head.as_deref() == Some(name.as_str()),
                target: target.to_string(),
                name,
            });
        }

        let remotes: Vec<String> = self
            .inner
            .remote_names()
            .iter()
            .map(|name| name.to_string())
            .collect();
        let mut remote_branches = Vec::new();
        for reference in platform
            .remote_branches()
            .map_err(wrap(Error::References))?
        {
            let mut reference = reference.map_err(Error::References)?;
            if matches!(reference.target(), gix::refs::TargetRef::Symbolic(_)) {
                continue;
            }
            let Ok(target) = reference.peel_to_id() else {
                continue;
            };
            let name = reference.name().shorten().to_string();
            remote_branches.push(RemoteBranch {
                remote: remote_of(&name, &remotes),
                target: target.to_string(),
                name,
            });
        }

        let mut tags = Vec::new();
        for reference in platform.tags().map_err(wrap(Error::References))? {
            let mut reference = reference.map_err(Error::References)?;
            let annotated = match reference.target() {
                gix::refs::TargetRef::Object(id) => self
                    .inner
                    .find_header(id)
                    .is_ok_and(|header| header.kind() == gix::object::Kind::Tag),
                gix::refs::TargetRef::Symbolic(_) => false,
            };
            let Ok(target) = reference.peel_to_id() else {
                continue;
            };
            tags.push(Tag {
                name: reference.name().shorten().to_string(),
                target: target.to_string(),
                annotated,
            });
        }

        Ok(Refs {
            branches,
            remote_branches,
            tags,
        })
    }
}

/// The configured remote a remote-tracking branch belongs to.
///
/// Remote names may themselves contain slashes, so `a/b/main` belongs to
/// `a/b` when both `a` and `a/b` are remotes: the longest match wins.
fn remote_of(branch: &str, remotes: &[String]) -> Option<String> {
    remotes
        .iter()
        .filter(|remote| {
            branch
                .strip_prefix(remote.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|remote| remote.len())
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::remote_of;

    #[test]
    fn the_longest_remote_name_owns_the_branch() {
        let remotes = vec!["a".to_owned(), "a/b".to_owned()];
        assert_eq!(remote_of("a/b/main", &remotes).as_deref(), Some("a/b"));
        assert_eq!(remote_of("a/main", &remotes).as_deref(), Some("a"));
    }

    #[test]
    fn a_prefix_that_is_not_a_whole_component_does_not_match() {
        let remotes = vec!["origin".to_owned()];
        assert_eq!(remote_of("origin2/main", &remotes), None);
    }
}
