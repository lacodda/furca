//! Where a repository stands, read from throwaway repositories built by the
//! system `git` in a temp directory: a working one, and a bare `origin` it
//! pushes to when a test needs a remote.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{Operation, Repository};

/// A temp directory holding `work`, and `origin.git` once a test asks for it.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        std::fs::create_dir_all(fixture.work()).expect("work dir");
        fixture.git(&["init", "--quiet", "--initial-branch=main"]);
        fixture
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    /// A bare `origin` that `work` pushes `main` to and follows.
    fn with_origin(self) -> Self {
        let origin = self.dir.path().join("origin.git");
        git_in(
            self.dir.path(),
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                "origin.git",
            ],
            DATE,
        );
        let origin = origin.to_str().expect("utf-8 path").to_owned();
        self.git(&["remote", "add", "origin", &origin]);
        self
    }

    fn git(&self, args: &[&str]) -> String {
        git_in(&self.work(), args, DATE)
    }

    /// Makes an empty commit dated `seconds` and returns its id.
    fn commit(&self, message: &str, seconds: i64) -> String {
        git_in(
            &self.work(),
            &["commit", "--quiet", "--allow-empty", "-m", message],
            &format!("{seconds} +0300"),
        );
        self.git(&["rev-parse", "HEAD"])
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.work().join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
        std::fs::write(path, text).expect("write");
    }

    fn open(&self) -> Repository {
        Repository::open(self.work()).expect("furca-core opens the fixture")
    }

    /// One file committed and pushed to `origin`, which `main` follows:
    /// settled, so that a test can unsettle it one way at a time.
    fn pushed(self) -> Self {
        let fixture = self.with_origin();
        fixture.write(
            ".gitignore",
            ".no-config
",
        );
        fixture.write(
            "a.txt", "one
",
        );
        fixture.git(&["add", "--all"]);
        fixture.git(&["commit", "--quiet", "-m", "begin"]);
        fixture.git(&["push", "--quiet", "--set-upstream", "origin", "main"]);
        let standing = fixture.open().standing().expect("standing");
        assert!(standing.settled(), "{standing:?}");
        fixture
    }
}

const DATE: &str = "1700000000 +0000";

/// Runs `git` shut off from the machine's configuration, with a fixed
/// identity and both dates pinned to `date`, and returns what it printed.
fn git_in(dir: &Path, args: &[&str], date: &str) -> String {
    let output = run_git(dir, args, date);
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

/// [`git_in`] for a run that is allowed to fail.
fn run_git(dir: &Path, args: &[&str], date: &str) -> std::process::Output {
    let empty = dir.join(".no-config");
    if !empty.exists() {
        std::fs::write(&empty, "").expect("empty config");
    }
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada Author")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada Author")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git runs")
}

#[test]
fn a_repository_without_commits_is_a_standing_with_nothing_committed() {
    let fixture = Fixture::new();
    let standing = fixture.open().standing().expect("standing reads");

    assert_eq!(standing.branch.as_deref(), Some("main"));
    assert_eq!(standing.commit, None);
    assert_eq!(standing.upstream, None);
    assert_eq!(standing.tag, None);
    assert_eq!(
        (standing.changed, standing.unpushed, standing.stashes),
        (0, 0, 0)
    );
    // The `.no-config` the fixture writes is the one untracked file, and
    // nothing else holds the repository back.
    assert_eq!(standing.untracked, 1);
    assert!(!standing.settled(), "an untracked file exists only here");
}

#[test]
fn each_kind_of_work_left_here_unsettles_a_repository_on_its_own() {
    // A change alone.
    let fixture = Fixture::new().pushed();
    fixture.write(
        "a.txt", "two
",
    );
    let standing = fixture.open().standing().expect("standing");
    assert_eq!((standing.changed, standing.untracked), (1, 0));
    assert!(!standing.settled());

    // An untracked file alone.
    let fixture = Fixture::new().pushed();
    fixture.write(
        "notes.txt",
        "scratch
",
    );
    let standing = fixture.open().standing().expect("standing");
    assert_eq!((standing.changed, standing.untracked), (0, 1));
    assert!(!standing.settled());

    // A stash alone.
    let fixture = Fixture::new().pushed();
    fixture.write(
        "a.txt", "two
",
    );
    fixture.git(&["stash", "--quiet"]);
    let standing = fixture.open().standing().expect("standing");
    assert_eq!((standing.changed, standing.stashes), (0, 1));
    assert!(!standing.settled());

    // An operation alone: a bisect changes no file.
    let fixture = Fixture::new().pushed();
    fixture.git(&["bisect", "start"]);
    let standing = fixture.open().standing().expect("standing");
    assert_eq!(standing.operation, Some(Operation::Bisect));
    assert_eq!((standing.changed, standing.untracked), (0, 0));
    assert!(!standing.settled());
}

#[test]
fn changes_count_once_per_path_and_untracked_directories_once_each() {
    let fixture = Fixture::new();
    fixture.write(".gitignore", ".no-config\n");
    fixture.write("a.txt", "one\n");
    fixture.write("b.txt", "one\n");
    fixture.write("c.txt", "one\n");
    fixture.git(&["add", "--all"]);
    fixture.git(&["commit", "--quiet", "-m", "begin"]);
    let standing = fixture.open().standing().expect("standing");
    assert_eq!((standing.changed, standing.untracked), (0, 0));

    // Staged and then changed again: one change.
    fixture.write("a.txt", "two\n");
    fixture.git(&["add", "a.txt"]);
    fixture.write("a.txt", "three\n");
    // Changed, not staged.
    fixture.write("b.txt", "two\n");
    // Deleted.
    std::fs::remove_file(fixture.work().join("c.txt")).expect("remove");
    // An untracked file, and a directory of two that counts once.
    fixture.write("notes.txt", "scratch\n");
    fixture.write("fresh/one.txt", "1\n");
    fixture.write("fresh/two.txt", "2\n");

    let standing = fixture.open().standing().expect("standing");
    assert_eq!(standing.changed, 3);
    assert_eq!(standing.untracked, 2);
    assert!(!standing.settled());
}

#[test]
fn ahead_behind_and_unpushed_are_read_against_the_remote_tracking_branches() {
    let fixture = Fixture::new().pushed();

    let standing = fixture.open().standing().expect("standing");
    let upstream = standing
        .upstream
        .as_ref()
        .expect("main follows origin/main");
    assert_eq!(upstream.name, "origin/main");
    assert_eq!((upstream.ahead, upstream.behind), (Some(0), Some(0)));
    assert_eq!(standing.unpushed, 0);

    // Someone else pushes one commit, and this clone fetches it.
    let other = fixture.dir.path().join("other");
    let origin = fixture.dir.path().join("origin.git");
    git_in(
        fixture.dir.path(),
        &[
            "clone",
            "--quiet",
            origin.to_str().expect("utf-8"),
            other.to_str().expect("utf-8"),
        ],
        DATE,
    );
    git_in(
        &other,
        &["commit", "--quiet", "--allow-empty", "-m", "theirs"],
        DATE,
    );
    git_in(&other, &["push", "--quiet", "origin", "main"], DATE);
    fixture.git(&["fetch", "--quiet", "origin"]);

    // Two commits here, on main.
    fixture.commit("mine 1", 1_700_000_100);
    fixture.commit("mine 2", 1_700_000_200);
    // One on a branch that was never pushed.
    fixture.git(&["switch", "--quiet", "-c", "topic"]);
    fixture.commit("topic", 1_700_000_300);
    fixture.git(&["switch", "--quiet", "main"]);

    let standing = fixture.open().standing().expect("standing");
    let upstream = standing.upstream.as_ref().expect("still follows");
    assert_eq!((upstream.ahead, upstream.behind), (Some(2), Some(1)));
    assert_eq!(standing.unpushed, 3, "main's two and topic's one");
    assert_eq!((standing.changed, standing.untracked), (0, 0));
    assert!(!standing.settled(), "unpushed commits exist only here");

    // A commit on a detached HEAD is on no branch, and is unpushed too.
    fixture.git(&["switch", "--quiet", "--detach"]);
    fixture.commit("adrift", 1_700_000_400);
    let standing = fixture.open().standing().expect("standing");
    assert_eq!(standing.branch, None);
    assert_eq!(standing.upstream, None, "a detached HEAD follows nothing");
    assert_eq!(standing.unpushed, 4);
}

#[test]
fn without_remotes_every_commit_is_unpushed() {
    let fixture = Fixture::new();
    fixture.commit("one", 1_700_000_000);
    fixture.commit("two", 1_700_000_100);
    assert_eq!(fixture.open().standing().expect("standing").unpushed, 2);
}

#[test]
fn an_upstream_whose_branch_is_gone_has_no_ahead_or_behind() {
    let fixture = Fixture::new().with_origin();
    fixture.commit("one", 1_700_000_000);
    fixture.git(&["config", "branch.main.remote", "origin"]);
    fixture.git(&["config", "branch.main.merge", "refs/heads/main"]);

    let standing = fixture.open().standing().expect("standing");
    let upstream = standing.upstream.expect("configured");
    assert_eq!(upstream.name, "origin/main");
    assert_eq!((upstream.ahead, upstream.behind), (None, None));
}

#[test]
fn the_nearest_tag_is_the_one_head_descends_from_with_the_commits_since() {
    let fixture = Fixture::new();
    let first = fixture.commit("one", 1_700_000_000);
    git_in(
        &fixture.work(),
        &["tag", "--annotate", "v0.1.0", "-m", "v0.1.0", &first],
        "1700000500 +0200",
    );
    fixture.commit("two", 1_700_001_000);
    let third = fixture.commit("three", 1_700_002_000);
    // A newer tag on a branch HEAD does not descend from is not its tag.
    fixture.git(&["switch", "--quiet", "-c", "side", &first]);
    let side = fixture.commit("side", 1_700_003_000);
    fixture.git(&["tag", "v9.0.0", &side]);
    fixture.git(&["switch", "--quiet", "main"]);

    let standing = fixture.open().standing().expect("standing");
    let tag = standing.tag.expect("v0.1.0 is in the history");
    assert_eq!(tag.name, "v0.1.0");
    assert_eq!(tag.since, 2);
    assert_eq!(
        tag.seconds, 1_700_000_500,
        "the tagger's time, not the commit's"
    );
    assert_eq!(tag.time, "2023-11-15T00:21:40+02:00");
    assert_eq!(
        standing.commit.map(|c| c.id).as_deref(),
        Some(third.as_str())
    );

    // A plain tag on HEAD is nearer still, and dates by its commit.
    fixture.git(&["tag", "v0.2.0"]);
    let tag = fixture
        .open()
        .standing()
        .expect("standing")
        .tag
        .expect("a tag");
    assert_eq!((tag.name.as_str(), tag.since), ("v0.2.0", 0));
    assert_eq!(tag.seconds, 1_700_002_000);
}

#[test]
fn stashes_are_counted_and_unsettle_the_repository() {
    let fixture = Fixture::new();
    fixture.write(".gitignore", ".no-config\n");
    fixture.write("a.txt", "one\n");
    fixture.git(&["add", "--all"]);
    fixture.git(&["commit", "--quiet", "-m", "begin"]);
    for text in ["two\n", "three\n"] {
        fixture.write("a.txt", text);
        fixture.git(&["stash", "--quiet"]);
    }

    let standing = fixture.open().standing().expect("standing");
    assert_eq!(standing.stashes, 2);
    assert_eq!(standing.changed, 0, "the changes went into the stash");
    assert!(!standing.settled());
}

#[test]
fn a_merge_left_halfway_is_named() {
    let fixture = Fixture::new();
    fixture.write(".gitignore", ".no-config\n");
    fixture.write("a.txt", "base\n");
    fixture.git(&["add", "--all"]);
    fixture.git(&["commit", "--quiet", "-m", "base"]);
    fixture.git(&["switch", "--quiet", "-c", "topic"]);
    fixture.write("a.txt", "topic\n");
    fixture.git(&["commit", "--quiet", "-am", "topic"]);
    fixture.git(&["switch", "--quiet", "main"]);
    fixture.write("a.txt", "main\n");
    fixture.git(&["commit", "--quiet", "-am", "main"]);
    let merged = run_git(&fixture.work(), &["merge", "--quiet", "topic"], DATE);
    assert!(!merged.status.success(), "the merge conflicts");
    assert!(fixture.work().join(".git").join("MERGE_HEAD").exists());

    let standing = fixture.open().standing().expect("standing");
    assert_eq!(standing.operation, Some(Operation::Merge));
    assert!(!standing.settled());
}

#[test]
fn a_pattern_finds_the_repositories_in_a_directory_and_nothing_inside_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    for name in ["alpha", "beta", "gamma-server"] {
        let path = dir.path().join(name);
        std::fs::create_dir_all(&path).expect("dir");
        git_in(&path, &["init", "--quiet", "--initial-branch=main"], DATE);
    }
    // Not a repository: matched by the pattern, and left out.
    std::fs::create_dir_all(dir.path().join("plain")).expect("dir");
    // A directory inside a repository is not one, even though it is in one.
    std::fs::create_dir_all(dir.path().join("alpha").join("src")).expect("dir");
    // A file is not a directory.
    std::fs::write(dir.path().join("loose.txt"), "").expect("file");

    let names = |pattern: PathBuf| -> Vec<String> {
        furca_core::repositories(pattern)
            .iter()
            .map(|root| {
                assert!(root.is_absolute(), "{root:?}");
                root.file_name()
                    .expect("a name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    };

    assert_eq!(
        names(dir.path().join("*")),
        ["alpha", "beta", "gamma-server"]
    );
    assert_eq!(names(dir.path().join("*-server")), ["gamma-server"]);
    assert_eq!(names(dir.path().join("?eta")), ["beta"]);
    assert!(names(dir.path().join("alpha").join("*")).is_empty());
    assert_eq!(names(dir.path().join("alpha")), ["alpha"]);
    assert!(names(dir.path().join("missing").join("*")).is_empty());
    // Through the git directory, the same repository, once.
    assert_eq!(names(dir.path().join("alpha").join(".git")), ["alpha"]);
}
