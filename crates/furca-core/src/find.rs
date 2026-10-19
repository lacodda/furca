use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// The repositories at the directories `pattern` matches, each named by
/// its working tree's root - or its git directory, when it is bare.
///
/// `*` matches any run of characters within one directory name and `?`
/// matches one, as in a shell; a name without them matches itself, and a
/// leading `.` only matches a `.` written out, so `*` passes over `.git`. A
/// match counts only if it is a repository itself: a directory inside
/// one is not walked up from, so `projects/*` finds the repositories in
/// `projects` and not the repository `projects` may sit in. Names compare
/// without case where the file system does, on Windows and macOS.
///
/// The paths come back absolute, in the order of the pattern's matches
/// sorted by name, each repository once however many matches lead to it.
/// A directory that cannot be read matches nothing.
pub fn repositories(pattern: impl AsRef<Path>) -> Vec<PathBuf> {
    let pattern = pattern.as_ref();
    let pattern = std::path::absolute(pattern).unwrap_or_else(|_| pattern.to_path_buf());

    let mut candidates = vec![PathBuf::new()];
    for component in pattern.components() {
        let Component::Normal(name) = component else {
            for candidate in &mut candidates {
                candidate.push(component);
            }
            continue;
        };
        if !wild(name) {
            for candidate in &mut candidates {
                candidate.push(name);
            }
            continue;
        }
        let pattern: Vec<char> = fold(name).chars().collect();
        let mut next = Vec::new();
        for candidate in &candidates {
            let Ok(entries) = std::fs::read_dir(candidate) else {
                continue;
            };
            let mut matched: Vec<PathBuf> = entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    let name: Vec<char> = fold(&entry.file_name()).chars().collect();
                    matches(&pattern, &name)
                })
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect();
            matched.sort();
            next.extend(matched);
        }
        candidates = next;
    }

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter_map(|candidate| root(&candidate))
        .filter(|root| seen.insert(std::fs::canonicalize(root).unwrap_or_else(|_| root.clone())))
        .collect()
}

/// The root of the repository at `dir` exactly; `None` when `dir` holds
/// none. The machine's configuration is not read: only the repository's
/// own says where its working tree is.
fn root(dir: &Path) -> Option<PathBuf> {
    let repo = gix::open_opts(dir, gix::open::Options::isolated()).ok()?;
    let root = repo.workdir().unwrap_or_else(|| repo.git_dir());
    Some(std::path::absolute(root).unwrap_or_else(|_| root.to_path_buf()))
}

fn wild(name: &OsStr) -> bool {
    name.to_string_lossy().contains(['*', '?'])
}

/// A name as it compares: without case where the file system ignores it.
fn fold(name: &OsStr) -> String {
    let name = name.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) {
        name.to_lowercase()
    } else {
        name.into_owned()
    }
}

fn matches(pattern: &[char], name: &[char]) -> bool {
    if name.first() == Some(&'.') && pattern.first() != Some(&'.') {
        return false;
    }
    wildcard(pattern, name)
}

fn wildcard(pattern: &[char], name: &[char]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some(('*', rest)) => (0..=name.len()).any(|skip| wildcard(rest, &name[skip..])),
        Some(('?', rest)) => !name.is_empty() && wildcard(rest, &name[1..]),
        Some((c, rest)) => name.first() == Some(c) && wildcard(rest, &name[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::matches;

    fn glob(pattern: &str, name: &str) -> bool {
        let pattern: Vec<char> = pattern.chars().collect();
        let name: Vec<char> = name.chars().collect();
        matches(&pattern, &name)
    }

    #[test]
    fn a_star_matches_any_run_and_a_question_mark_one_character() {
        assert!(glob("*", "furca"));
        assert!(glob("*", ""));
        assert!(glob("f*a", "furca"));
        assert!(glob("f*a", "fa"));
        assert!(!glob("f*a", "furcas"));
        assert!(glob("*-server", "kasl-server"));
        assert!(!glob("*-server", "kasl"));
        assert!(glob("k??l", "kasl"));
        assert!(!glob("k??l", "kal"));
        assert!(glob("plain", "plain"));
        assert!(!glob("plain", "plainer"));
    }

    #[test]
    fn a_leading_dot_is_matched_only_when_written_out() {
        assert!(!glob("*", ".git"));
        assert!(!glob("?git", ".git"));
        assert!(glob(".*", ".git"));
        assert!(glob("*.git", "repo.git"));
    }
}
