//! The release's section of the changelog, written by `git-cliff-core` from
//! the repository's own `cliff.toml` and put under the changelog's header.
//!
//! Only the new section is rendered. Everything already in the file - the
//! header, the sections of earlier releases, whatever was written there by
//! hand - is kept byte for byte: the line never regenerates a changelog
//! whole, because a regenerated file loses what a person added to it.

use std::path::Path;

use furca_core::Repository;
use git_cliff_core::changelog::Changelog;
use git_cliff_core::commit::{Commit, Signature};
use git_cliff_core::config::Config;
use git_cliff_core::release::Release;

use crate::error::Error;

/// The configuration git-cliff reads at the root of a repository.
pub const CLIFF: &str = "cliff.toml";

/// Renders the section for `tag` from the commits since `since` (the last
/// release's tag; the whole history for a first release), dated `seconds`
/// after the epoch. Empty when the configuration shows none of the commits.
pub(crate) fn section(
    root: &Path,
    repo: &Repository,
    since: Option<&str>,
    tag: &str,
    seconds: i64,
) -> Result<String, Error> {
    let mut config = config(root)?;
    // The header is already in the file and stays as it is there.
    config.changelog.header = None;
    config.changelog.footer = None;

    let mut commits = Vec::new();
    let mut head = None;
    for commit in repo.history(since)? {
        let commit = commit?;
        head.get_or_insert_with(|| commit.id.clone());
        let message = repo.message(&commit.id)?;
        let mut cliff = Commit::new(commit.id.clone(), message);
        cliff.author = Signature {
            name: Some(commit.author.name),
            email: Some(commit.author.email),
            timestamp: commit.author.seconds,
        };
        cliff.committer = Signature {
            name: Some(commit.committer.name),
            email: Some(commit.committer.email),
            timestamp: commit.committer.seconds,
        };
        cliff.merge_commit = commit.parents.len() > 1;
        commits.push(cliff);
    }
    // The history comes newest first; git-cliff itself turns it around
    // unless the configuration asks for the newest on top.
    if config.git.sort_commits != "newest" {
        commits.reverse();
    }

    let release = Release {
        version: Some(tag.to_owned()),
        commits,
        commit_id: head,
        timestamp: Some(seconds),
        previous: Some(Box::new(Release {
            version: since.map(str::to_owned),
            ..Release::default()
        })),
        repository: Some(root.to_string_lossy().into_owned()),
        ..Release::default()
    };
    let changelog = Changelog::new(vec![release], config, None).map_err(cliff)?;
    let mut out = Vec::new();
    changelog.generate(&mut out).map_err(cliff)?;
    Ok(String::from_utf8_lossy(&out).trim().to_owned())
}

/// The repository's `cliff.toml`, or git-cliff's own default without one.
fn config(root: &Path) -> Result<Config, Error> {
    let path = root.join(CLIFF);
    match std::fs::read_to_string(&path) {
        Ok(text) => text.parse::<Config>().map_err(|e| {
            Error::Config(vec![format!(
                "`{CLIFF}` is not a git-cliff configuration: {e}"
            )])
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            git_cliff_core::embed::EmbeddedConfig::parse().map_err(cliff)
        }
        Err(source) => Err(Error::Read { path, source }),
    }
}

fn cliff(error: git_cliff_core::error::Error) -> Error {
    Error::Changelog(error.to_string())
}

/// `changelog` with `section` put above the first release's section - right
/// under the header - with one blank line on each side. Nothing else in the
/// text moves.
pub(crate) fn insert(changelog: &str, section: &str) -> String {
    let newline = if changelog.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let section = section.replace("\r\n", "\n").replace('\n', newline);
    let mut at = None;
    let mut offset = 0;
    for line in changelog.split_inclusive('\n') {
        if line.starts_with("## ") {
            at = Some(offset);
            break;
        }
        offset += line.len();
    }
    match at {
        Some(at) => format!(
            "{}{section}{newline}{newline}{}",
            &changelog[..at],
            &changelog[at..]
        ),
        None => {
            let head = changelog.trim_end();
            if head.is_empty() {
                format!("{section}{newline}")
            } else {
                format!("{head}{newline}{newline}{section}{newline}")
            }
        }
    }
}

/// Whether `changelog` has a section for `version`, as `## [1.2.3]`,
/// `## 1.2.3` or `## v1.2.3` with anything after.
pub(crate) fn has_section(changelog: &str, version: &str) -> bool {
    changelog
        .lines()
        .filter_map(|line| line.strip_prefix("## "))
        .any(|heading| {
            heading
                .trim_start_matches('[')
                .split([']', ' '])
                .next()
                .map(|n| n.trim_start_matches('v'))
                == Some(version)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str =
        "# Changelog\n\nWritten by hand.\n\n## [0.3.0] - 2026-10-05\n\n### Features\n- Plan\n";

    #[test]
    fn the_section_goes_under_the_header_and_nothing_else_moves() {
        let out = insert(OLD, "## [0.4.0] - 2026-10-11\n\n### Features\n- Run");
        assert_eq!(
            out,
            "# Changelog\n\nWritten by hand.\n\n## [0.4.0] - 2026-10-11\n\n### Features\n- Run\n\n## [0.3.0] - 2026-10-05\n\n### Features\n- Plan\n"
        );
        assert!(has_section(&out, "0.4.0") && has_section(&out, "0.3.0"));
        assert!(!has_section(&out, "0.4"));
    }

    #[test]
    fn a_first_section_follows_the_header_and_keeps_its_line_endings() {
        let out = insert("# Changelog\r\n\r\nNotes.\r\n", "## [0.1.0]\n\n- First");
        assert_eq!(
            out,
            "# Changelog\r\n\r\nNotes.\r\n\r\n## [0.1.0]\r\n\r\n- First\r\n"
        );
        assert_eq!(insert("", "## [0.1.0]"), "## [0.1.0]\n");
    }
}
