//! The `furca` binary end to end, on repositories built by `git` in a temp
//! directory.

use std::path::Path;
use std::process::{Command, Output};

fn git(dir: &Path, args: &[&str]) {
    let empty = dir.join(".no-config");
    std::fs::write(&empty, "").expect("empty config");
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada Author")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada Author")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", "1700000000 +0000")
        .env("GIT_COMMITTER_DATE", "1700000000 +0000")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn repo_with(commits: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    for message in commits {
        git(
            dir.path(),
            &["commit", "--quiet", "--allow-empty", "-m", message],
        );
    }
    dir
}

fn furca(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_furca"))
        .args(args)
        .arg("-C")
        .arg(dir)
        .output()
        .expect("furca runs")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "furca failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("utf-8")
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid JSON")
}

#[test]
fn status_speaks_to_a_person_and_to_a_program() {
    let dir = repo_with(&["first"]);

    let text = stdout(&furca(dir.path(), &["status"]));
    assert!(text.starts_with("On main at "), "{text}");

    let value = json(&furca(dir.path(), &["status", "--json"]));
    assert_eq!(value["branch"], "main");
    assert_eq!(value["detached"], false);
    assert!(value["commit"].as_str().is_some_and(|id| id.len() == 40));
    assert!(value["upstream"].is_null());
}

#[test]
fn an_empty_repository_is_a_state_not_a_failure() {
    let dir = repo_with(&[]);

    assert_eq!(
        stdout(&furca(dir.path(), &["status"])).trim(),
        "On main, no commits yet"
    );
    assert_eq!(
        stdout(&furca(dir.path(), &["log"])).trim(),
        "No commits yet"
    );
    assert_eq!(stdout(&furca(dir.path(), &["refs"])).trim(), "No refs yet");

    let log = json(&furca(dir.path(), &["log", "--json"]));
    assert_eq!(log["commits"].as_array().map(Vec::len), Some(0));
    assert_eq!(log["truncated"], false);
}

#[test]
fn log_honours_the_limit_and_says_when_it_cut() {
    let dir = repo_with(&["one", "two", "three"]);

    let text = stdout(&furca(dir.path(), &["log", "-n", "2"]));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 3, "{text}");
    assert!(lines[0].ends_with("  three"), "{text}");
    assert!(lines[0].contains(" 2023-11-14 Ada Author "), "{text}");
    assert!(lines[2].starts_with("... more history"), "{text}");

    let value = json(&furca(dir.path(), &["log", "--limit", "2", "--json"]));
    assert_eq!(value["commits"].as_array().map(Vec::len), Some(2));
    assert_eq!(value["truncated"], true);
    assert_eq!(value["commits"][0]["subject"], "three");
    assert_eq!(
        value["commits"][0]["parents"][0], value["commits"][1]["id"],
        "the second commit is the first one's parent"
    );
}

#[test]
fn refs_marks_the_current_branch() {
    let dir = repo_with(&["first"]);
    git(dir.path(), &["branch", "other"]);
    git(dir.path(), &["tag", "v1"]);

    let text = stdout(&furca(dir.path(), &["refs"]));
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("* ") && lines[0].ends_with(" main"),
        "{text}"
    );
    assert!(
        lines[1].starts_with("  ") && lines[1].ends_with(" other"),
        "{text}"
    );
    assert!(lines[2].ends_with(" tag: v1"), "{text}");

    let value = json(&furca(dir.path(), &["refs", "--json"]));
    assert_eq!(
        value["branches"][0]["name"], "main",
        "branches sort by name"
    );
    assert_eq!(value["branches"][0]["head"], true);
    assert_eq!(value["branches"][1]["name"], "other");
    assert_eq!(value["branches"][1]["head"], false);
    assert_eq!(value["tags"][0]["annotated"], false);
}

#[test]
fn outside_a_repository_it_fails_with_a_message_and_a_nonzero_exit() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = furca(&dir.path().join("missing"), &["status"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("furca: could not open the repository"),
        "{stderr}"
    );
    assert!(output.stdout.is_empty(), "nothing half-printed on stdout");
}

#[test]
fn an_error_exits_two_so_a_script_can_tell_it_from_a_no() {
    let dir = tempfile::tempdir().expect("temp dir");
    let output = furca(&dir.path().join("missing"), &["status"]);
    assert_eq!(output.status.code(), Some(2));
}

/// A project with a `release.toml`, released at v0.1.0 a moment ago and with
/// one feature since. Dated now, so its dependency update is fresh.
fn released_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    let write = |path: &str, text: &str| {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().expect("a parent")).expect("dirs");
        std::fs::write(full, text).expect("write");
    };
    write(
        "Cargo.toml",
        "[package]\nname = \"plan-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\ndescription = \"A fixture.\"\nreadme = \"README.md\"\n",
    );
    write("README.md", "# plan-fixture\n\nPin it with `v0.1.0`.\n");
    write("CHANGELOG.md", "# Changelog\n\n## [0.1.0] - 2026-01-01\n");
    write(".github/workflows/ci.yml", "name: CI\n");
    write(
        "release.toml",
        "[project]\nname = \"plan-fixture\"\nchangelog = \"CHANGELOG.md\"\nmanifests = [\"Cargo.toml\"]\n\n[gate]\ncommands = [\"cargo test\"]\n\n[ci]\nprovider = \"github\"\nworkflow = \"ci.yml\"\n",
    );
    let now = format!(
        "{} +0000",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_secs()
    );
    let git_now = |args: &[&str]| {
        let empty = dir.path().join(".no-config");
        std::fs::write(&empty, "").expect("empty config");
        let status = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Author")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Author")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", &now)
            .env("GIT_COMMITTER_DATE", &now)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?} failed");
    };
    git_now(&["init", "--quiet", "--initial-branch=main"]);
    std::fs::write(dir.path().join(".gitignore"), ".no-config\n").expect("ignore");
    git_now(&["add", "--all"]);
    git_now(&["commit", "--quiet", "-m", "chore(deps): pin the toolchain"]);
    git_now(&["tag", "v0.1.0"]);
    git_now(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "feat: a command",
    ]);
    dir
}

#[test]
fn release_plan_proposes_the_next_number_and_exits_zero_when_ready() {
    let dir = released_project();

    let value = json(&furca(dir.path(), &["release", "plan", "--json"]));
    assert_eq!(value["ready"], true, "{value:#}");
    assert_eq!(value["released"], "0.1.0");
    assert_eq!(value["proposed"]["version"], "0.2.0");
    assert_eq!(value["steps"]["tag"], "v0.2.0");

    let output = furca(dir.path(), &["release", "plan"]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).expect("utf-8");
    assert!(
        text.starts_with("plan-fixture 0.1.0 -> 0.2.0, a minor step"),
        "{text}"
    );
    assert!(
        text.contains("README.md line 3: v0.1.0 -> v0.2.0"),
        "{text}"
    );
    assert!(
        text.trim_end()
            .ends_with("Ready: nothing stands in the way."),
        "{text}"
    );
}

#[test]
fn release_plan_exits_one_when_something_stands_in_the_way() {
    let dir = released_project();
    std::fs::write(
        dir.path().join("README.md"),
        "# plan-fixture\n\nPin it with `v0.0.9`.\n",
    )
    .expect("write");

    let output = furca(dir.path(), &["release", "plan"]);
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).expect("utf-8");
    assert!(text.contains("README.md:3: v0.0.9"), "{text}");
    assert!(
        text.trim_end().ends_with("Not ready: texts fails."),
        "{text}"
    );
}

#[test]
fn release_plan_without_release_toml_says_so_and_exits_two() {
    let dir = repo_with(&["first"]);
    let output = furca(dir.path(), &["release", "plan"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("furca: no release.toml in "), "{stderr}");
}
