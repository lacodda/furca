//! furca-core's mutations - through the system `git` - against throwaway
//! repositories: a working repository and a bare one it pushes to, both in a
//! temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{Error, Repository};

/// A working repository with an `origin` that is a bare repository next to
/// it. The working repository's own configuration shuts out the machine's:
/// no global hooks, no signing, a fixed identity - the `git` furca runs reads
/// the global configuration, and a local value wins over it.
struct Pair {
    dir: tempfile::TempDir,
}

impl Pair {
    fn new() -> Pair {
        let pair = Pair {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        std::fs::create_dir_all(pair.work()).expect("work dir");
        std::fs::create_dir_all(pair.dir.path().join("no-hooks")).expect("hooks dir");
        git(
            pair.dir.path(),
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                "origin.git",
            ],
        );
        git(&pair.work(), &["init", "--quiet", "--initial-branch=main"]);
        let hooks = pair.dir.path().join("no-hooks");
        for (key, value) in [
            ("user.name", "Ada Author"),
            ("user.email", "ada@example.com"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.autocrlf", "false"),
            ("core.hooksPath", hooks.to_str().expect("utf-8 path")),
        ] {
            git(&pair.work(), &["config", key, value]);
        }
        let origin = pair.dir.path().join("origin.git");
        git(
            &pair.work(),
            &[
                "remote",
                "add",
                "origin",
                origin.to_str().expect("utf-8 path"),
            ],
        );
        pair.write("README.md", "one\n");
        git(&pair.work(), &["add", "--all"]);
        git(&pair.work(), &["commit", "--quiet", "-m", "chore: begin"]);
        git(
            &pair.work(),
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );
        pair
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.work().join(path), text).expect("write");
    }

    fn open(&self) -> Repository {
        Repository::open(self.work()).expect("furca-core opens the repository")
    }

    fn origin(&self, args: &[&str]) -> String {
        git(&self.dir.path().join("origin.git"), args)
    }
}

/// The test's own `git`, with the machine's configuration shut out.
fn git(dir: &Path, args: &[&str]) -> String {
    let empty = std::env::temp_dir().join("furca-core-tests.gitconfig");
    if !empty.exists() {
        std::fs::write(&empty, "").expect("empty config");
    }
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("utf-8")
        .trim()
        .to_owned()
}

#[test]
fn the_upstream_names_the_remote_its_address_and_the_branch_there() {
    let pair = Pair::new();
    let upstream = pair
        .open()
        .upstream()
        .expect("reads")
        .expect("main follows origin/main");
    assert_eq!(upstream.branch, "main");
    assert_eq!(upstream.remote, "origin");
    assert_eq!(upstream.merge, "refs/heads/main");
    assert_eq!(upstream.tracking.as_deref(), Some("origin/main"));
    assert!(upstream.url.ends_with("origin.git"), "{}", upstream.url);

    git(&pair.work(), &["checkout", "--quiet", "-b", "topic"]);
    assert_eq!(
        pair.open().upstream().expect("reads"),
        None,
        "topic follows nothing"
    );
    git(&pair.work(), &["checkout", "--quiet", "--detach"]);
    assert_eq!(pair.open().upstream().expect("reads"), None, "detached");
}

#[test]
fn changed_paths_are_tracked_files_whose_content_moved() {
    let pair = Pair::new();
    let repo = pair.open();
    assert!(repo.changed_paths().expect("reads").is_empty());

    pair.write("untracked.txt", "new\n");
    assert!(
        repo.changed_paths().expect("reads").is_empty(),
        "an untracked file is no change"
    );

    // Same content written again: the timestamp moves, the content does not.
    pair.write("README.md", "one\n");
    assert!(repo.changed_paths().expect("reads").is_empty());

    pair.write("README.md", "two\n");
    assert_eq!(repo.changed_paths().expect("reads"), ["README.md"]);

    pair.write("staged.txt", "staged\n");
    git(&pair.work(), &["add", "staged.txt"]);
    assert_eq!(
        repo.changed_paths().expect("reads"),
        ["README.md", "staged.txt"]
    );
}

#[test]
fn a_commit_a_tag_and_a_push_go_through_git_and_into_the_log() {
    let pair = Pair::new();
    let repo = pair.open();
    pair.write("README.md", "two\n");
    pair.write("other.txt", "left out\n");
    git(&pair.work(), &["add", "other.txt"]);
    git(&pair.work(), &["reset", "--quiet"]);

    repo.git().add(&["README.md"]).expect("adds");
    let id = repo
        .git()
        .commit("chore(release): v0.2.0")
        .expect("commits");
    assert_eq!(
        repo.head().expect("head").commit.as_deref(),
        Some(id.as_str())
    );
    assert_eq!(
        repo.commit(&id).expect("reads").subject,
        "chore(release): v0.2.0"
    );
    assert!(
        repo.changed_paths().expect("reads").is_empty(),
        "only what was added went in; other.txt is untracked"
    );

    repo.git().tag("v0.2.0", "v0.2.0", &id).expect("tags");
    let tags = repo.refs().expect("refs").tags;
    assert_eq!(tags.len(), 1);
    assert!(tags[0].annotated);
    assert_eq!(tags[0].target, id);

    repo.git()
        .push("origin", &["HEAD:refs/heads/main", "refs/tags/v0.2.0"])
        .expect("pushes");
    assert_eq!(pair.origin(&["rev-parse", "main"]), id);
    assert_eq!(
        repo.git()
            .remote_ref("origin", "refs/tags/v0.2.0")
            .expect("asks"),
        Some(id.clone()),
        "an annotated tag on the remote is peeled to its commit"
    );
    assert_eq!(
        repo.git()
            .remote_ref("origin", "refs/tags/v9.9.9")
            .expect("asks"),
        None
    );

    let log: Vec<String> = repo.git_log().iter().map(|run| run.command()).collect();
    let tag = format!("git tag --annotate v0.2.0 --message v0.2.0 {id}");
    assert_eq!(
        log,
        [
            "git add -- README.md",
            "git commit --quiet -m \"chore(release): v0.2.0\"",
            "git rev-parse HEAD",
            tag.as_str(),
            "git push --quiet origin HEAD:refs/heads/main refs/tags/v0.2.0",
            "git ls-remote origin refs/tags/v0.2.0 refs/tags/v0.2.0^{}",
            "git ls-remote origin refs/tags/v9.9.9 refs/tags/v9.9.9^{}",
        ]
    );
}

#[test]
fn a_failing_git_says_what_it_ran_and_what_git_said() {
    let pair = Pair::new();
    let repo = pair.open();
    let error = repo
        .git()
        .commit("nothing staged")
        .expect_err("nothing to commit");
    match &error {
        Error::Git { command, code, .. } => {
            assert_eq!(command, "git commit --quiet -m \"nothing staged\"");
            assert_eq!(*code, Some(1));
        }
        other => panic!("not a git failure: {other:?}"),
    }
    assert!(error.to_string().starts_with("`git commit"), "{error}");
}

#[test]
fn an_ancestor_is_reachable_and_unrelated_history_is_not() {
    let pair = Pair::new();
    let first = git(&pair.work(), &["rev-parse", "HEAD"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "--allow-empty", "-m", "fix: second"],
    );
    let second = git(&pair.work(), &["rev-parse", "HEAD"]);
    let repo = pair.open();
    assert!(repo.is_ancestor(&first, &second).expect("reads"));
    assert!(repo.is_ancestor(&second, &second).expect("reads"));
    assert!(!repo.is_ancestor(&second, &first).expect("reads"));

    git(&pair.work(), &["checkout", "--quiet", "--orphan", "other"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "--allow-empty", "-m", "chore: apart"],
    );
    let apart = git(&pair.work(), &["rev-parse", "HEAD"]);
    assert!(!pair.open().is_ancestor(&first, &apart).expect("reads"));
}
