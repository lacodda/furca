//! nooma, when it is installed: the line's search keeps a history of a
//! repository's commits as documents, and finds a commit by the words of its
//! whole message - the body where the reason for a change is written, which
//! furca's own search of subjects does not read.
//!
//! nooma is asked through its command line, `nooma repo history --json`,
//! whose output its documentation calls a contract. Without nooma the window
//! searches on its own and says nothing is missing.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde::Deserialize;

/// How many commits from the top of the history nooma reads: its own
/// default, said out loud so a change there does not change furca.
const LIMIT: &str = "5000";

/// The nooma on the PATH, looked for once.
pub fn installed() -> Option<&'static Path> {
    static FOUND: OnceLock<Option<PathBuf>> = OnceLock::new();
    FOUND
        .get_or_init(|| std::env::var_os("PATH").and_then(|path| lookup(&path)))
        .as_deref()
}

/// The nooma executable in the directories of `path`, a PATH value.
fn lookup(path: &OsStr) -> Option<PathBuf> {
    let name = if cfg!(windows) { "nooma.exe" } else { "nooma" };
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[derive(Deserialize)]
struct History {
    commits: Vec<Document>,
}

#[derive(Deserialize)]
struct Document {
    id: String,
}

/// The commits of the repository at `repository` whose message holds
/// `text`, as nooma finds them: full hex ids, newest first.
pub fn history(nooma: &Path, repository: &str, text: &str) -> Result<Vec<String>, String> {
    let mut command = Command::new(nooma);
    command.args([
        "repo",
        "history",
        repository,
        "--matching",
        text,
        "--limit",
        LIMIT,
        "--json",
    ]);
    // A console program started from a window gets a console window of its
    // own on Windows, which would flash at every search.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let output = command
        .output()
        .map_err(|e| format!("nooma did not start: {e}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let reason = said
            .lines()
            .rfind(|line| !line.trim().is_empty())
            .unwrap_or("");
        return Err(format!(
            "nooma could not read the history: {}",
            reason.trim()
        ));
    }
    parse(&output.stdout)
}

fn parse(json: &[u8]) -> Result<Vec<String>, String> {
    let history: History = serde_json::from_slice(json)
        .map_err(|e| format!("nooma answered in a shape furca does not read: {e}"))?;
    Ok(history
        .commits
        .into_iter()
        .map(|commit| commit.id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{lookup, parse};

    /// The example in nooma's own reference for `repo history --json`.
    const DOCUMENTED: &str = r#"{
      "head": "29028610740cc923baf966791b1e2090597fa546",
      "commits": [
        {
          "id": "29028610740cc923baf966791b1e2090597fa546",
          "summary": "feat: add double entry",
          "body": "Lines sum to zero in every currency.",
          "author": "A Writer",
          "time": 1789856815,
          "paths": ["src/ledger.rs"]
        }
      ]
    }"#;

    #[test]
    fn the_documented_answer_reads_as_its_commits() {
        assert_eq!(
            parse(DOCUMENTED.as_bytes()).unwrap(),
            ["29028610740cc923baf966791b1e2090597fa546"]
        );
        assert!(parse(b"{}").is_err(), "a shape without commits is refused");
    }

    #[test]
    fn nooma_is_found_in_a_directory_of_the_path_and_only_there() {
        let empty = tempfile::tempdir().unwrap();
        let holding = tempfile::tempdir().unwrap();
        let name = if cfg!(windows) { "nooma.exe" } else { "nooma" };
        std::fs::write(holding.path().join(name), "").unwrap();

        let path = std::env::join_paths([empty.path(), holding.path()]).unwrap();
        assert_eq!(lookup(&path), Some(holding.path().join(name)));
        let path = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(lookup(&path), None);
    }
}
