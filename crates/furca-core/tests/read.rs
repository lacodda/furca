//! furca-core against real repositories built by the system `git` in a temp
//! directory — never against a repository on the author's machine.

use std::path::Path;
use std::process::Command;

use furca_core::{Repository, Tips};

/// A throwaway repository with a `git` that ignores the machine's own
/// configuration: no global hooks, no signing, no default branch surprises.
struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        fixture.git(&["init", "--quiet", "--initial-branch=main"]);
        fixture
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_at(args, "1700000000 +0000")
    }

    /// Runs `git` with author and committer dates pinned to `date`
    /// (`<unix seconds> <offset>`), so ordering by time is under the test's
    /// control.
    fn git_at(&self, args: &[&str], date: &str) -> String {
        let empty = self.path().join(".no-config");
        std::fs::write(&empty, "").expect("empty config");
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Author")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Carl Committer")
            .env("GIT_COMMITTER_EMAIL", "carl@example.com")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
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

    /// Makes an empty commit at `seconds` and returns its id.
    fn commit(&self, message: &str, seconds: i64) -> String {
        self.git_at(
            &["commit", "--quiet", "--allow-empty", "-m", message],
            &format!("{seconds} +0300"),
        );
        self.git(&["rev-parse", "HEAD"])
    }

    fn open(&self) -> Repository {
        Repository::open(self.path()).expect("furca-core opens the fixture")
    }
}

#[test]
fn a_repository_without_commits_reads_as_empty_not_as_an_error() {
    let fixture = Fixture::new();
    let repo = fixture.open();

    let head = repo.head().expect("HEAD reads before the first commit");
    assert_eq!(head.branch.as_deref(), Some("main"));
    assert_eq!(head.commit, None);
    assert!(!head.detached);

    let log = repo.log(Tips::All, 10).expect("an empty history walks");
    assert!(log.commits.is_empty());
    assert!(!log.truncated, "nothing was cut off an empty history");

    let refs = repo.refs().expect("refs read");
    assert!(
        refs.branches.is_empty(),
        "an unborn branch has no commit to list"
    );
}

#[test]
fn open_discovers_the_repository_from_a_subdirectory() {
    let fixture = Fixture::new();
    let nested = fixture.path().join("a").join("b");
    std::fs::create_dir_all(&nested).expect("nested dirs");
    let first = fixture.commit("first", 1_700_000_000);

    let repo = Repository::open(&nested).expect("discovered upwards");
    assert_eq!(
        repo.head().expect("head").commit.as_deref(),
        Some(first.as_str())
    );
}

#[test]
fn a_linear_log_is_newest_first_with_parents_people_and_subjects() {
    let fixture = Fixture::new();
    let first = fixture.commit("first\n\nbody that is not the subject", 1_700_000_000);
    let second = fixture.commit("second", 1_700_000_100);

    let log = fixture.open().log(Tips::Head, 10).expect("log");
    let ids: Vec<&str> = log.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, [second.as_str(), first.as_str()]);
    assert!(!log.truncated);

    let top = &log.commits[0];
    assert_eq!(top.parents, [first.as_str()]);
    assert_eq!(top.subject, "second");
    assert_eq!(log.commits[1].subject, "first");
    assert!(log.commits[1].parents.is_empty());

    assert_eq!(top.author.name, "Ada Author");
    assert_eq!(top.author.email, "ada@example.com");
    assert_eq!(top.committer.name, "Carl Committer");
    // The person's own offset is kept, not converted to UTC or local time.
    assert_eq!(top.author.time, "2023-11-15T01:15:00+03:00");
}

#[test]
fn the_limit_cuts_the_walk_and_says_so() {
    let fixture = Fixture::new();
    for n in 0..5 {
        fixture.commit(&format!("c{n}"), 1_700_000_000 + n);
    }

    let log = fixture.open().log(Tips::Head, 3).expect("log");
    assert_eq!(log.commits.len(), 3);
    assert!(log.truncated);

    let exact = fixture.open().log(Tips::Head, 5).expect("log");
    assert_eq!(exact.commits.len(), 5);
    assert!(
        !exact.truncated,
        "a limit equal to the history cuts nothing"
    );

    let none = fixture.open().log(Tips::Head, 0).expect("log");
    assert!(none.commits.is_empty());
    assert!(none.truncated);
}

/// A clock that ran backwards: the child is recorded as older than its
/// parent. A merge then queues both at once, and a walk that sorts by time
/// alone would list the parent first — a graph drawn from that would have an
/// edge pointing up.
#[test]
fn a_parent_never_comes_before_its_child_even_when_clocks_disagree() {
    let fixture = Fixture::new();
    let root = fixture.commit("root", 1_700_000_000);
    let parent = fixture.commit("parent from the future", 1_800_000_000);
    let child = fixture.commit("child with a slow clock", 1_700_000_500);
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"]);
    let merge = fixture.git_at(
        &[
            "commit-tree",
            &tree,
            "-p",
            &child,
            "-p",
            &parent,
            "-m",
            "merge",
        ],
        "1700000600 +0000",
    );
    fixture.git(&["update-ref", "refs/heads/main", &merge]);

    let log = fixture.open().log(Tips::Head, 10).expect("log");
    let ids: Vec<&str> = log.commits.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            merge.as_str(),
            child.as_str(),
            parent.as_str(),
            root.as_str()
        ]
    );
}

#[test]
fn a_merge_lists_its_mainline_first_and_all_starts_from_every_ref() {
    let fixture = Fixture::new();
    let base = fixture.commit("base", 1_700_000_000);
    fixture.git(&["checkout", "--quiet", "-b", "side"]);
    let side = fixture.commit("side work", 1_700_000_100);
    fixture.git(&["checkout", "--quiet", "main"]);
    let main = fixture.commit("main work", 1_700_000_200);
    fixture.git_at(
        &["merge", "--quiet", "--no-ff", "-m", "merge side", "side"],
        "1700000300 +0000",
    );
    let merge = fixture.git(&["rev-parse", "HEAD"]);

    // A branch HEAD cannot reach.
    fixture.git(&["checkout", "--quiet", "-b", "orphan-work", &base]);
    let unreachable = fixture.commit("only on orphan-work", 1_700_000_400);
    fixture.git(&["checkout", "--quiet", "main"]);

    let repo = fixture.open();
    let from_head = repo.log(Tips::Head, 50).expect("log");
    let merge_commit = &from_head.commits[0];
    assert_eq!(merge_commit.id, merge);
    assert_eq!(merge_commit.parents, [main.clone(), side.clone()]);
    assert!(
        !from_head.commits.iter().any(|c| c.id == unreachable),
        "HEAD's log does not wander onto other branches"
    );

    let all = repo.log(Tips::All, 50).expect("log");
    assert!(all.commits.iter().any(|c| c.id == unreachable));
    assert_eq!(
        all.commits.iter().filter(|c| c.id == base).count(),
        1,
        "a commit reachable from several tips is listed once"
    );
    for (index, commit) in all.commits.iter().enumerate() {
        for parent in &commit.parents {
            let at = all.commits.iter().position(|c| &c.id == parent);
            assert!(
                at.is_some_and(|at| at > index),
                "parent {parent} of {} is listed before it",
                commit.id
            );
        }
    }
}

#[test]
fn refs_list_branches_remotes_and_tags_with_what_they_point_at() {
    let fixture = Fixture::new();
    let first = fixture.commit("first", 1_700_000_000);
    let second = fixture.commit("second", 1_700_000_100);
    fixture.git(&["branch", "feature/login", &first]);
    fixture.git(&["tag", "light", &first]);
    fixture.git(&["tag", "-a", "v1.0.0", "-m", "release", &second]);
    let tree = fixture.git(&["rev-parse", "HEAD^{tree}"]);
    fixture.git(&["tag", "a-tree", &tree]);

    fixture.git(&["remote", "add", "origin", "https://example.com/repo.git"]);
    fixture.git(&["update-ref", "refs/remotes/origin/main", &first]);
    fixture.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    fixture.git(&["config", "branch.main.remote", "origin"]);
    fixture.git(&["config", "branch.main.merge", "refs/heads/main"]);

    let repo = fixture.open();
    let refs = repo.refs().expect("refs");

    let branches: Vec<(&str, &str, Option<&str>, bool)> = refs
        .branches
        .iter()
        .map(|b| {
            (
                b.name.as_str(),
                b.target.as_str(),
                b.upstream.as_deref(),
                b.head,
            )
        })
        .collect();
    assert_eq!(
        branches,
        [
            ("feature/login", first.as_str(), None, false),
            ("main", second.as_str(), Some("origin/main"), true),
        ]
    );

    assert_eq!(refs.remote_branches.len(), 1, "origin/HEAD is not listed");
    let remote = &refs.remote_branches[0];
    assert_eq!(remote.name, "origin/main");
    assert_eq!(remote.remote.as_deref(), Some("origin"));
    assert_eq!(remote.target, first);

    let tags: Vec<(&str, &str, bool)> = refs
        .tags
        .iter()
        .map(|t| (t.name.as_str(), t.target.as_str(), t.annotated))
        .collect();
    assert_eq!(
        tags,
        [
            ("a-tree", tree.as_str(), false),
            ("light", first.as_str(), false),
            ("v1.0.0", second.as_str(), true),
        ],
        "an annotated tag is peeled to its commit"
    );

    let head = repo.head().expect("head");
    assert_eq!(head.upstream.as_deref(), Some("origin/main"));

    // A tag pointing at a tree is a ref, but not somewhere a walk can start.
    let all = repo.log(Tips::All, 50).expect("log with a tree tag");
    assert_eq!(all.commits.len(), 2);
}

#[test]
fn a_detached_head_names_no_branch() {
    let fixture = Fixture::new();
    let first = fixture.commit("first", 1_700_000_000);
    fixture.commit("second", 1_700_000_100);
    fixture.git(&["checkout", "--quiet", "--detach", &first]);

    let repo = fixture.open();
    let head = repo.head().expect("head");
    assert!(head.detached);
    assert_eq!(head.branch, None);
    assert_eq!(head.upstream, None);
    assert_eq!(head.commit.as_deref(), Some(first.as_str()));
    assert!(
        repo.refs().expect("refs").branches.iter().all(|b| !b.head),
        "no branch is current while detached"
    );
}
