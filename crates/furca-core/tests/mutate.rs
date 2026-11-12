//! furca-core's mutations - through the system `git` - against throwaway
//! repositories: a working repository and a bare one it pushes to, both in a
//! temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{Error, FileChange, Hunk, LineRange, Picked, Repository, Status};

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

    repo.git().tag("v0.2.0", &id, Some("v0.2.0")).expect("tags");
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
    let tag = format!("git tag --annotate --file=- -- v0.2.0 {id}");
    assert_eq!(
        log,
        [
            "git --literal-pathspecs add --pathspec-from-file=- --pathspec-file-nul",
            "git commit --quiet --file=-",
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
            assert_eq!(command, "git commit --quiet --file=-");
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

fn modified<'a>(status: &'a Status, path: &str) -> &'a FileChange {
    status
        .unstaged
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is not in {status:#?}"))
}

fn staged<'a>(status: &'a Status, path: &str) -> &'a FileChange {
    status
        .staged
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is not in {status:#?}"))
}

/// What the index holds at `path`.
fn in_index(pair: &Pair, path: &str) -> String {
    let shown = Command::new("git")
        .current_dir(pair.work())
        .args(["show", &format!(":{path}")])
        .output()
        .expect("git runs");
    String::from_utf8(shown.stdout).expect("utf-8")
}

/// A file of ten lines in `HEAD`, with lines 2 and 9 changed in the working
/// tree: two hunks far enough apart that a patch of one says nothing of the other.
fn two_hunks(pair: &Pair, path: &str) -> Repository {
    let lines = |two: &str, nine: &str| format!("1\n{two}\n3\n4\n5\n6\n7\n8\n{nine}\n10\n");
    pair.write(path, &lines("2", "9"));
    git(&pair.work(), &["add", "--", path]);
    git(
        &pair.work(),
        &["commit", "--quiet", "-m", "chore: ten lines"],
    );
    pair.write(path, &lines("TWO", "NINE"));
    pair.open()
}

#[test]
fn staging_one_hunk_takes_that_hunk_and_leaves_the_file_and_the_other_hunk() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    let status = repo.status().expect("status");
    let file = modified(&status, "ten.txt");
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 2, "{diff:#?}");

    repo.stage_lines(file, &[Picked::whole(diff.hunks[1])])
        .expect("stages");
    assert_eq!(
        in_index(&pair, "ten.txt"),
        "1\n2\n3\n4\n5\n6\n7\n8\nNINE\n10\n"
    );
    assert_eq!(
        std::fs::read_to_string(pair.work().join("ten.txt")).expect("reads"),
        "1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\n10\n",
        "the working tree is not touched"
    );

    // And what is staged comes out again the same way, one hunk at a time.
    let status = pair.open().status().expect("status");
    let file = staged(&status, "ten.txt").clone();
    let repo = pair.open();
    let diff = repo.diff(&file).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "{diff:#?}");
    repo.unstage_lines(&file, &[Picked::whole(diff.hunks[0])])
        .expect("unstages");
    assert_eq!(
        in_index(&pair, "ten.txt"),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"
    );
}

#[test]
fn a_hunk_the_file_no_longer_has_is_refused_and_the_index_stays() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    let status = repo.status().expect("status");
    let file = modified(&status, "ten.txt");
    let hunk = repo.diff(file).expect("diff").hunks[1];

    // Saved again in between, and the hunk has moved.
    pair.write("ten.txt", "new\n1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\n10\n");
    match repo.stage_lines(file, &[Picked::whole(hunk)]) {
        Err(Error::Moved { path }) => assert_eq!(path, "ten.txt"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        in_index(&pair, "ten.txt"),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"
    );
}

#[test]
fn a_hunk_is_put_in_byte_for_byte() {
    // Bytes that are not UTF-8, carriage returns, a last line with no newline,
    // and a name a patch has to quote: what the window's text cannot carry.
    let pair = Pair::new();
    let path = "dir name/é 'q'.txt";
    std::fs::create_dir_all(pair.work().join("dir name")).expect("dir");
    let mut before = Vec::new();
    for line in 1..=9 {
        before.extend_from_slice(line.to_string().as_bytes());
        before.extend_from_slice(b"\xFF\r\n");
    }
    before.extend_from_slice(b"last \xE9");
    let file_on_disk = pair.work().join(path);
    std::fs::write(&file_on_disk, &before).expect("writes");
    git(&pair.work(), &["add", "--", path]);
    git(&pair.work(), &["commit", "--quiet", "-m", "chore: bytes"]);

    let mut after = before.clone();
    // Line 2 changes, and so does the last one.
    let second = b"2\xFF\r\n";
    let at = after
        .windows(second.len())
        .position(|bytes| bytes == second)
        .expect("line 2");
    after.splice(at..at + second.len(), b"TWO \xFE\r\n".iter().copied());
    let end = after.len() - b"last \xE9".len();
    after.splice(end.., b"LAST \xE9".iter().copied());
    std::fs::write(&file_on_disk, &after).expect("writes");

    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = modified(&status, path);
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 2, "{diff:#?}");
    repo.stage_lines(file, &[Picked::whole(diff.hunks[1])])
        .expect("stages");

    let mut expected = before.clone();
    let end = expected.len() - b"last \xE9".len();
    expected.splice(end.., b"LAST \xE9".iter().copied());
    let shown = Command::new("git")
        .current_dir(pair.work())
        .args(["show", &format!(":{path}")])
        .output()
        .expect("git runs");
    assert_eq!(shown.stdout, expected);
}

#[test]
fn a_hunk_added_after_a_last_line_with_no_newline_ends_that_line() {
    let pair = Pair::new();
    pair.write("a.txt", "x\ny");
    git(&pair.work(), &["add", "--", "a.txt"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "-m", "chore: no newline"],
    );
    pair.write("a.txt", "x\ny\nz\n");

    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = modified(&status, "a.txt");
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "{diff:#?}");
    repo.stage_lines(file, &[Picked::whole(diff.hunks[0])])
        .expect("stages");
    assert_eq!(in_index(&pair, "a.txt"), "x\ny\nz\n");
}

#[test]
fn a_file_added_whole_has_no_hunks_to_take() {
    let pair = Pair::new();
    pair.write("new.txt", "a\n");
    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = status
        .untracked
        .iter()
        .find(|file| file.path == "new.txt")
        .expect("untracked");
    let hunk = Hunk {
        before: LineRange { start: 0, count: 0 },
        after: LineRange { start: 0, count: 1 },
    };
    assert!(matches!(
        repo.stage_lines(file, &[Picked::whole(hunk)]),
        Err(Error::NotByHunks { .. })
    ));
}

#[test]
fn a_path_is_taken_as_it_is_and_never_as_a_pattern() {
    let pair = Pair::new();
    // `[a].txt` as a pattern matches `a.txt`.
    pair.write(
        "[a].txt", "one
",
    );
    pair.write(
        "a.txt", "two
",
    );
    let repo = pair.open();
    repo.git().add(&["[a].txt"]).expect("adds");
    let status = repo.status().expect("status");
    let names: Vec<&str> = status.staged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(names, ["[a].txt"], "{status:#?}");

    // `:(top)` is pathspec magic, and no name a file can have on Windows.
    if cfg!(unix) {
        pair.write(
            ":(top)b.txt",
            "three
",
        );
        let repo = pair.open();
        repo.git().add(&[":(top)b.txt"]).expect("adds");
        repo.git().reset(&["[a].txt"]).expect("resets");
        let status = repo.status().expect("status");
        let names: Vec<&str> = status.staged.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(names, [":(top)b.txt"], "{status:#?}");
    }
}

#[test]
fn staging_everything_or_only_what_git_tracks_and_taking_it_back() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "tracked.txt");
    pair.write("new.txt", "a\n");

    repo.git().add_tracked().expect("adds");
    let status = repo.status().expect("status");
    assert_eq!(status.staged.len(), 1, "{status:#?}");
    assert_eq!(status.untracked.len(), 1, "{status:#?}");

    let repo = pair.open();
    repo.git().add_all().expect("adds");
    let status = repo.status().expect("status");
    assert_eq!(status.staged.len(), 2, "{status:#?}");
    assert!(status.untracked.is_empty(), "{status:#?}");

    let repo = pair.open();
    repo.git().reset_all().expect("resets");
    let status = repo.status().expect("status");
    assert!(status.staged.is_empty(), "{status:#?}");
    assert_eq!(status.unstaged.len(), 1, "{status:#?}");
    assert_eq!(status.untracked.len(), 1, "{status:#?}");
}

#[test]
fn no_paths_is_nothing_to_do_and_not_every_path() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "tracked.txt");
    pair.write("new.txt", "a\n");
    repo.git().add(&[]).expect("nothing to do");
    repo.git().reset(&[]).expect("nothing to do");
    assert!(repo.git_log().is_empty(), "{:?}", repo.git_log());
    let status = repo.status().expect("status");
    assert!(status.staged.is_empty(), "{status:#?}");
}

#[test]
fn a_commit_before_the_first_commit_can_be_unstaged_and_committed() {
    let pair = Pair::new();
    let unborn = pair.dir.path().join("unborn");
    std::fs::create_dir_all(&unborn).expect("dir");
    git(&unborn, &["init", "--quiet", "--initial-branch=main"]);
    for (key, value) in [
        ("user.name", "Ada Author"),
        ("user.email", "ada@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&unborn, &["config", key, value]);
    }
    std::fs::write(unborn.join("first.txt"), "a\n").expect("writes");
    let repo = Repository::at(&unborn).expect("opens");
    repo.git().add(&["first.txt"]).expect("adds");
    repo.git().reset(&["first.txt"]).expect("resets");
    let status = repo.status().expect("status");
    assert!(status.staged.is_empty(), "{status:#?}");
    repo.git().add_all().expect("adds");
    let id = repo.git().commit("feat: the first").expect("commits");
    assert_eq!(git(&unborn, &["rev-parse", "HEAD"]), id);
}

#[test]
fn a_long_message_goes_in_whole_and_amend_takes_the_place_of_the_last_commit() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    let body = "A line of the body that says why.\n".repeat(3000);
    let message = format!("feat: a large commit\n\n{body}");
    repo.git().add_tracked().expect("adds");
    let first = repo.git().commit(&message).expect("commits");
    assert_eq!(
        git(&pair.work(), &["log", "-1", "--format=%B"]).trim(),
        message.trim()
    );
    let parent = git(&pair.work(), &["rev-parse", "HEAD~1"]);

    let second = repo.git().amend("fix: a small commit").expect("amends");
    assert_ne!(first, second);
    assert_eq!(git(&pair.work(), &["rev-parse", "HEAD"]), second);
    assert_eq!(git(&pair.work(), &["rev-parse", "HEAD~1"]), parent);
    assert_eq!(
        git(&pair.work(), &["log", "-1", "--format=%s"]),
        "fix: a small commit"
    );
}

#[test]
fn a_hook_that_refuses_a_commit_is_heard_in_its_own_words() {
    let pair = Pair::new();
    let hooks = pair.dir.path().join("hooks");
    std::fs::create_dir_all(&hooks).expect("dir");
    let hook = hooks.join("commit-msg");
    std::fs::write(
        &hook,
        "#!/bin/sh\nif grep -q forbidden \"$1\"; then\n  echo 'that word is not allowed' >&2\n  exit 1\nfi\n",
    )
    .expect("writes");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let repo = two_hunks(&pair, "ten.txt");

    assert_eq!(
        repo.git().commit_msg_says("feat: fine").expect("asks"),
        None,
        "no hook configured yet"
    );
    git(
        &pair.work(),
        &["config", "core.hooksPath", hooks.to_str().expect("utf-8")],
    );
    let repo = pair.open();
    assert_eq!(
        repo.git().commit_msg_says("feat: fine").expect("asks"),
        None
    );
    let says = repo
        .git()
        .commit_msg_says("feat: a forbidden word")
        .expect("asks");
    assert_eq!(says.as_deref(), Some("that word is not allowed"));

    // Asked and nothing left behind, and no run of it in the log: nothing changed.
    let left: Vec<_> = std::fs::read_dir(repo.git_dir())
        .expect("reads")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("FURCA_"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(repo.git_log().is_empty(), "{:?}", repo.git_log());

    // The commit itself hears the same hook, in the same words.
    repo.git().add_tracked().expect("adds");
    match repo.git().commit("feat: a forbidden word") {
        Err(Error::Git { stderr, .. }) => {
            assert!(stderr.contains("that word is not allowed"), "{stderr}")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_lock_on_the_index_held_for_a_moment_is_waited_out() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    let lock = repo.git_dir().join("index.lock");
    std::fs::write(&lock, "").expect("takes the lock");
    let letting_go = {
        let lock = lock.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            std::fs::remove_file(&lock).expect("lets go");
        })
    };
    repo.git().add_tracked().expect("waits and adds");
    letting_go.join().expect("joined");
    assert_eq!(repo.status().expect("status").staged.len(), 1);
}

#[test]
fn a_run_keeps_when_it_was_what_went_in_and_what_git_said() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    repo.git()
        .add(&["ten.txt", "nothing.txt"])
        .expect_err("no such file");
    let runs = repo.git_log();
    let run = runs.last().expect("a run");
    assert_eq!(run.input, "ten.txt\nnothing.txt");
    assert!(run.said.contains("nothing.txt"), "{run:?}");
    assert_ne!(run.code, Some(0));
    assert!(run.at > 1_600_000_000_000, "{run:?}");
}

/// What most people on Windows have: `\n` in the index and `\r\n` on disk. The
/// hunk has to go in as the index keeps it, or a clean file comes back as every
/// line changed.
#[test]
fn a_hunk_goes_in_as_the_index_keeps_its_lines_under_autocrlf() {
    let pair = Pair::new();
    git(&pair.work(), &["config", "core.autocrlf", "true"]);
    let lines = |two: &str, nine: &str| format!("1\n{two}\n3\n4\n5\n6\n7\n8\n{nine}\n10\n");
    pair.write("ten.txt", &lines("2", "9"));
    git(&pair.work(), &["add", "--", "ten.txt"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "-m", "chore: ten lines"],
    );
    pair.write("ten.txt", &lines("TWO", "NINE").replace('\n', "\r\n"));

    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = modified(&status, "ten.txt");
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 2, "{diff:#?}");

    repo.stage_lines(file, &[Picked::whole(diff.hunks[0])])
        .expect("stages");
    assert_eq!(in_index(&pair, "ten.txt"), lines("TWO", "9"));
}

/// A file of four lines in `HEAD` whose middle two are changed in the working
/// tree: one hunk of two lines removed and two added.
fn one_block(pair: &Pair) -> Repository {
    pair.write("four.txt", "1\n2\n3\n4\n");
    git(&pair.work(), &["add", "--", "four.txt"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "-m", "chore: four lines"],
    );
    pair.write("four.txt", "1\nTWO\nTHREE\n4\n");
    pair.open()
}

#[test]
fn picking_lines_takes_those_lines_and_leaves_the_rest_of_the_hunk() {
    let pair = Pair::new();
    let repo = one_block(&pair);
    let status = repo.status().expect("status");
    let file = modified(&status, "four.txt");
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "{diff:#?}");
    let hunk = diff.hunks[0];

    // 2 becomes TWO; 3 and THREE are left as they are.
    let first = Picked {
        hunk,
        removed: vec![1],
        added: vec![1],
    };
    repo.stage_lines(file, &[first]).expect("stages");
    assert_eq!(in_index(&pair, "four.txt"), "1\nTWO\n3\n4\n");

    // What is left is the other line, and it is one hunk of its own now.
    let repo = pair.open();
    let status = repo.status().expect("status");
    let diff = repo.diff(modified(&status, "four.txt")).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "{diff:#?}");
    assert_eq!(diff.hunks[0].before, LineRange { start: 2, count: 1 });

    // A line outside the hunk it is said to be in is not taken.
    let file = staged(&status, "four.txt").clone();
    let hunk = repo.diff(&file).expect("diff").hunks[0];
    let outside = Picked {
        hunk,
        removed: vec![],
        added: vec![3],
    };
    assert!(matches!(
        repo.unstage_lines(&file, &[outside]),
        Err(Error::Moved { .. })
    ));
    // Taking out only the line it added leaves the line it removed gone.
    let added = Picked {
        hunk,
        removed: vec![],
        added: vec![1],
    };
    repo.unstage_lines(&file, &[added]).expect("unstages");
    assert_eq!(in_index(&pair, "four.txt"), "1\n3\n4\n");
    // And the line it removed comes back on its own.
    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = staged(&status, "four.txt").clone();
    let hunk = repo.diff(&file).expect("diff").hunks[0];
    repo.unstage_lines(&file, &[Picked::whole(hunk)])
        .expect("unstages");
    assert_eq!(in_index(&pair, "four.txt"), "1\n2\n3\n4\n");
}

#[test]
fn discarding_lines_puts_them_back_as_the_index_has_them_and_leaves_the_rest() {
    let pair = Pair::new();
    let repo = one_block(&pair);
    let status = repo.status().expect("status");
    let file = modified(&status, "four.txt");
    let hunk = repo.diff(file).expect("diff").hunks[0];
    let first = Picked {
        hunk,
        removed: vec![1],
        added: vec![1],
    };
    repo.discard_lines(file, &[first]).expect("discards");
    assert_eq!(
        std::fs::read_to_string(pair.work().join("four.txt")).expect("reads"),
        "1\n2\nTHREE\n4\n"
    );
    assert_eq!(
        in_index(&pair, "four.txt"),
        "1\n2\n3\n4\n",
        "the index is not touched"
    );

    // A hunk the file has moved away from is refused, and the file stays.
    pair.write("four.txt", "0\n1\n2\nTHREE\n4\n");
    assert!(matches!(
        repo.discard_lines(file, &[Picked::whole(hunk)]),
        Err(Error::Moved { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(pair.work().join("four.txt")).expect("reads"),
        "0\n1\n2\nTHREE\n4\n"
    );
}

/// Discarding goes through the same conversion as a checkout: `\r\n` on disk
/// stays `\r\n` under autocrlf, and a line put back has one too.
#[test]
fn discarding_lines_keeps_the_endings_the_file_has_on_disk() {
    let pair = Pair::new();
    git(&pair.work(), &["config", "core.autocrlf", "true"]);
    let lines = |two: &str, nine: &str| format!("1\n{two}\n3\n4\n5\n6\n7\n8\n{nine}\n10\n");
    pair.write("ten.txt", &lines("2", "9"));
    git(&pair.work(), &["add", "--", "ten.txt"]);
    git(
        &pair.work(),
        &["commit", "--quiet", "-m", "chore: ten lines"],
    );
    pair.write("ten.txt", &lines("TWO", "NINE").replace('\n', "\r\n"));

    let repo = pair.open();
    let status = repo.status().expect("status");
    let file = modified(&status, "ten.txt");
    let diff = repo.diff(file).expect("diff");
    assert_eq!(diff.hunks.len(), 2, "{diff:#?}");
    repo.discard_lines(file, &[Picked::whole(diff.hunks[0])])
        .expect("discards");
    assert_eq!(
        std::fs::read_to_string(pair.work().join("ten.txt")).expect("reads"),
        lines("2", "NINE").replace('\n', "\r\n")
    );
}

#[test]
fn discarding_files_restores_the_tracked_and_deletes_the_untracked_as_named() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    pair.write("[a].txt", "untracked\n");
    pair.write("a.txt", "untracked too\n");
    std::fs::create_dir_all(pair.work().join("deep/er")).expect("dirs");
    pair.write("deep/er/x.txt", "x\n");

    repo.git().restore(&["ten.txt"]).expect("restores");
    assert_eq!(
        std::fs::read_to_string(pair.work().join("ten.txt")).expect("reads"),
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"
    );
    // A name with a pattern in it is that file, and no other.
    repo.git()
        .clean(&["[a].txt", "deep/er/x.txt"])
        .expect("cleans");
    assert!(!pair.work().join("[a].txt").exists());
    assert!(!pair.work().join("deep/er/x.txt").exists());
    assert!(pair.work().join("a.txt").exists());

    pair.write("README.md", "changed\n");
    repo.git().restore_all().expect("restores all");
    repo.git().clean_all().expect("cleans all");
    let status = pair.open().status().expect("status");
    assert!(status.unstaged.is_empty(), "{status:#?}");
    assert!(status.untracked.is_empty(), "{status:#?}");
}

#[test]
fn a_stash_puts_everything_away_and_gives_it_back() {
    let pair = Pair::new();
    let repo = two_hunks(&pair, "ten.txt");
    git(&pair.work(), &["add", "--", "ten.txt"]);
    pair.write("README.md", "changed\n");
    pair.write("new.txt", "untracked\n");

    let stash = repo
        .git()
        .stash(Some("  half done  "))
        .expect("stashes")
        .expect("there was something to put away");
    assert_eq!(stash.index, 0);
    assert_eq!(stash.message, "On main: half done");
    let status = pair.open().status().expect("status");
    assert!(
        status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty(),
        "{status:#?}"
    );

    // What it holds: the tracked changes, and the untracked file as added.
    let held = repo.changes(&stash.commit).expect("changes");
    let paths: Vec<&str> = held.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["README.md", "new.txt", "ten.txt"]);
    let new = &held.files[1];
    assert_eq!(new.status, furca_core::FileStatus::Added);
    assert_eq!(
        repo.diff(new).expect("diff").after.as_deref(),
        Some("untracked\n")
    );

    // Nothing left to put away is no entry, and no error.
    assert_eq!(repo.git().stash(None).expect("asks"), None);

    // Applied, it is there still; popped, it is gone.
    repo.git().stash_apply(&stash).expect("applies");
    assert_eq!(repo.stashes().expect("stashes").len(), 1);
    assert!(pair.work().join("new.txt").exists());
    repo.git().restore_all().expect("restores");
    repo.git().clean_all().expect("cleans");
    repo.git().stash_pop(&stash).expect("pops");
    assert!(repo.stashes().expect("stashes").is_empty());
    assert_eq!(
        std::fs::read_to_string(pair.work().join("README.md")).expect("reads"),
        "changed\n"
    );
    assert!(pair.work().join("new.txt").exists());
}

#[test]
fn a_stash_entry_that_moved_is_not_the_one_dropped() {
    let pair = Pair::new();
    let repo = pair.open();
    pair.write("README.md", "first\n");
    let first = repo.git().stash(None).expect("stashes").expect("made");
    pair.write("README.md", "second\n");
    repo.git().stash(None).expect("stashes").expect("made");

    // `first` was stash@{0}; it is stash@{1} now, and stash@{0} is another.
    assert!(matches!(
        repo.git().stash_drop(&first),
        Err(Error::Moved { .. })
    ));
    assert_eq!(repo.stashes().expect("stashes").len(), 2);
    let now = repo.stashes().expect("stashes").remove(1);
    assert_eq!(now.commit, first.commit);
    repo.git().stash_drop(&now).expect("drops");
    let left = repo.stashes().expect("stashes");
    assert_eq!(left.len(), 1);
    assert_ne!(left[0].commit, first.commit);
}

#[test]
fn a_stash_that_meets_a_conflict_says_where() {
    let pair = Pair::new();
    let repo = pair.open();
    pair.write("README.md", "stashed\n");
    let stash = repo.git().stash(None).expect("stashes").expect("made");
    pair.write("README.md", "committed\n");
    git(
        &pair.work(),
        &["commit", "--quiet", "-am", "chore: meanwhile"],
    );

    match repo.git().stash_pop(&stash) {
        Err(Error::Git { stderr, .. }) => assert!(stderr.contains("README.md"), "{stderr}"),
        other => panic!("{other:?}"),
    }
    // A pop that met a conflict keeps the entry, as git does.
    assert_eq!(repo.stashes().expect("stashes").len(), 1);
    assert_eq!(
        pair.open().status().expect("status").conflicted.len(),
        1,
        "the conflict is left to be resolved"
    );
}
