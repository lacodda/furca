//! The working copy against what git says of it, on repositories built by
//! `git` in a temporary directory: `git status --porcelain=v2` is the truth
//! both the whole read and a refresh by paths are held to.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use furca_core::{
    ConflictKind, Content, FileChange, FileMode, FileStatus, Refreshed, Repository, Status,
    StatusCache,
};

fn git_output(dir: &Path, args: &[&str]) -> String {
    let empty = dir.join(".git").join("no-global-config");
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Ada Author")
        .env("GIT_AUTHOR_EMAIL", "ada@example.com")
        .env("GIT_COMMITTER_NAME", "Ada Author")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn git(dir: &Path, args: &[&str]) {
    git_output(dir, args);
}

fn write(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).expect("create folders");
    }
    std::fs::write(file, content).expect("write the file");
}

/// A repository with a few committed files, a folder among them.
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    git(dir.path(), &["init", "--quiet", "--initial-branch=main"]);
    write(dir.path(), "a.txt", "one\ntwo\nthree\n");
    write(dir.path(), "b.txt", "bee\n");
    write(dir.path(), "src/lib.rs", "pub fn lib() {}\n");
    write(dir.path(), "src/main.rs", "fn main() {}\n");
    write(dir.path(), "src/deep/er.rs", "// deeper\n");
    write(dir.path(), ".gitignore", "target/\n*.log\n");
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "--quiet", "-m", "first"]);
    dir
}

/// One row: the list it is in, how it changed, where it is.
type Row = (&'static str, char, String);

/// What `git status` says, as rows.
fn rows_of_git(dir: &Path) -> BTreeSet<Row> {
    let text = git_output(
        dir,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=none",
        ],
    );
    let mut rows = BTreeSet::new();
    let mut fields = text.split('\0');
    while let Some(line) = fields.next() {
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        match parts[0] {
            "?" => {
                rows.insert(("untracked", 'A', parts[1].to_owned()));
            }
            "1" | "2" => {
                let fixed = if parts[0] == "1" { 7 } else { 8 };
                let columns: Vec<&str> = parts[1].splitn(fixed + 1, ' ').collect();
                let path = columns[fixed].to_owned();
                let states: Vec<char> = columns[0].chars().collect();
                if states[0] != '.' {
                    rows.insert(("staged", states[0], path.clone()));
                }
                if states[1] != '.' {
                    rows.insert(("unstaged", states[1], path.clone()));
                }
                if parts[0] == "2" {
                    // The path it came from, in a field of its own.
                    fields.next();
                }
            }
            "u" => {
                let columns: Vec<&str> = parts[1].splitn(10, ' ').collect();
                rows.insert(("conflicted", '!', columns[9].to_owned()));
            }
            other => panic!("unknown porcelain line {other}"),
        }
    }
    rows
}

fn letter(status: FileStatus) -> char {
    match status {
        FileStatus::Added => 'A',
        FileStatus::Deleted => 'D',
        FileStatus::Modified => 'M',
        FileStatus::Renamed => 'R',
        FileStatus::Copied => 'C',
    }
}

fn rows_of(status: &Status) -> BTreeSet<Row> {
    let mut rows = BTreeSet::new();
    let lists: [(&'static str, &Vec<FileChange>); 3] = [
        ("staged", &status.staged),
        ("unstaged", &status.unstaged),
        ("untracked", &status.untracked),
    ];
    for (list, files) in lists {
        for file in files {
            rows.insert((list, letter(file.status), file.path.clone()));
        }
    }
    for conflicted in &status.conflicted {
        rows.insert(("conflicted", '!', conflicted.path.clone()));
    }
    rows
}

#[track_caller]
fn assert_agrees_with_git(dir: &Path) {
    let repo = Repository::open(dir).expect("open");
    let status = repo.status().expect("read the status");
    assert_eq!(rows_of(&status), rows_of_git(dir));
}

fn read_all(dir: &Path) -> Status {
    let repo = Repository::open(dir).expect("open");
    let mut cache = StatusCache::read(&repo).expect("read the status");
    while cache.read_files(&repo, 16) > 0 {}
    assert!(!cache.has_unread());
    cache.status().clone()
}

#[test]
fn a_clean_tree_has_nothing_to_say() {
    let dir = repo();
    let repo = Repository::open(dir.path()).expect("open");
    let status = repo.status().expect("read the status");
    assert!(status.is_clean(), "{status:?}");
}

#[test]
fn each_list_says_what_git_says() {
    let dir = repo();
    let path = dir.path();
    // Staged: changed, added, deleted.
    write(path, "a.txt", "one\nTWO\nthree\n");
    write(path, "new.txt", "new\n");
    std::fs::remove_file(path.join("b.txt")).expect("remove");
    git(path, &["add", "-A"]);
    // Staged and then changed again, in both lists.
    write(path, "a.txt", "one\nTWO\nthree\nfour\n");
    // Unstaged: changed, deleted.
    write(path, "src/lib.rs", "pub fn changed() {}\n");
    std::fs::remove_file(path.join("src/main.rs")).expect("remove");
    // Untracked, each file on its own; a folder of them is not one row.
    write(path, "notes.txt", "notes\n");
    write(path, "docs/one.md", "one\n");
    write(path, "docs/more/two.md", "two\n");
    // Ignored: no row.
    write(path, "target/debug/out", "built\n");
    write(path, "run.log", "log\n");

    assert_agrees_with_git(path);
    let status = Repository::open(path).unwrap().status().unwrap();
    let names = |files: &[FileChange]| files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
    assert_eq!(names(&status.staged), ["a.txt", "b.txt", "new.txt"]);
    assert_eq!(
        names(&status.unstaged),
        ["a.txt", "src/lib.rs", "src/main.rs"]
    );
    assert_eq!(
        names(&status.untracked),
        ["docs/more/two.md", "docs/one.md", "notes.txt"]
    );
}

#[test]
fn a_status_is_read_before_any_file_is_opened() {
    let dir = repo();
    write(dir.path(), "a.txt", "changed\n");
    let status = Repository::open(dir.path()).unwrap().status().unwrap();
    let side = status.unstaged[0].after.as_ref().expect("a side on disk");
    assert_eq!(side.content, Content::Unread);
    assert_eq!(side.id, None, "git has stored no such file yet");
    assert_eq!(side.size, None);
    assert_eq!(status.unstaged[0].lines, None);
    assert!(status.unstaged[0].before.as_ref().unwrap().id.is_some());
}

#[test]
fn a_rename_is_staged_as_one_row_and_on_disk_as_two() {
    let dir = repo();
    let path = dir.path();
    git(path, &["mv", "b.txt", "bee.txt"]);
    assert_agrees_with_git(path);
    let status = read_all(path);
    assert_eq!(status.staged.len(), 1);
    assert_eq!(status.staged[0].status, FileStatus::Renamed);
    assert_eq!(status.staged[0].from.as_deref(), Some("b.txt"));
    assert_eq!(status.staged[0].similarity, Some(100));

    // On disk it is a deletion and an untracked file until it is staged.
    let dir = repo();
    let path = dir.path();
    std::fs::rename(path.join("b.txt"), path.join("moved.txt")).expect("move");
    assert_agrees_with_git(path);
}

#[test]
fn a_rename_with_changes_says_how_alike_the_file_stayed() {
    let dir = repo();
    let path = dir.path();
    let lines: String = (0..20).map(|n| format!("line number {n}\n")).collect();
    write(path, "long.txt", &lines);
    git(path, &["add", "."]);
    git(path, &["commit", "--quiet", "-m", "long"]);
    git(path, &["mv", "long.txt", "longer.txt"]);
    write(
        path,
        "longer.txt",
        &lines.replace("number 3\n", "NUMBER 3\n"),
    );
    git(path, &["add", "."]);

    let status = read_all(path);
    assert_eq!(status.staged[0].status, FileStatus::Renamed);
    let similarity = status.staged[0].similarity.expect("a figure once read");
    assert!((90..100).contains(&similarity), "{similarity}");
}

#[test]
fn reading_says_what_each_side_holds_and_how_large() {
    let dir = repo();
    let path = dir.path();
    // Staged: a binary and a git LFS pointer.
    std::fs::write(path.join("logo.png"), b"\x89PNG\0binary").expect("write");
    let oid = "a".repeat(64);
    write(
        path,
        "big.bin",
        &format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 1048576\n"),
    );
    git(path, &["add", "logo.png", "big.bin"]);
    // Unstaged: text, with its lines. Untracked: a binary.
    write(path, "a.txt", "one\nTWO\nthree\nfour\n");
    std::fs::write(path.join("photo.raw"), [0u8; 64]).expect("write");

    let status = read_all(path);
    let staged = |name: &str| {
        status
            .staged
            .iter()
            .find(|file| file.path == name)
            .and_then(|file| file.after.as_ref())
            .expect("a staged file")
    };
    assert_eq!(staged("logo.png").content, Content::Binary);
    assert_eq!(staged("logo.png").size, Some(11));
    assert_eq!(staged("big.bin").content, Content::Lfs { oid });
    assert_eq!(staged("big.bin").size, Some(1_048_576));

    let text = &status.unstaged[0];
    assert_eq!(text.path, "a.txt");
    let lines = text.lines.expect("text has lines");
    assert_eq!((lines.added, lines.removed), (2, 1));
    assert!(matches!(
        text.after.as_ref().unwrap().content,
        Content::Text { .. }
    ));

    let untracked = &status.untracked[0];
    assert_eq!(untracked.path, "photo.raw");
    assert_eq!(untracked.after.as_ref().unwrap().content, Content::Binary);
    assert_eq!(untracked.after.as_ref().unwrap().size, Some(64));
    assert_eq!(untracked.lines, None);
}

#[test]
fn the_diff_of_each_list_is_the_diff_git_prints() {
    let dir = repo();
    let path = dir.path();
    write(path, "a.txt", "one\nTWO\nthree\n");
    git(path, &["add", "a.txt"]);
    write(path, "a.txt", "one\nTWO\nthree\nfour\n");
    write(path, "notes.txt", "note one\nnote two\n");
    let repo = Repository::open(path).unwrap();
    let status = repo.status().unwrap();

    let patch_lines = |file: &FileChange| {
        let patch = repo.diff(file).expect("diff").patch(3);
        let mut lines = Vec::new();
        for hunk in &patch.hunks {
            for line in &hunk.lines {
                let mark = match line.kind {
                    furca_core::LineKind::Context => ' ',
                    furca_core::LineKind::Removed => '-',
                    furca_core::LineKind::Added => '+',
                };
                lines.push(format!("{mark}{}", line.text));
            }
        }
        lines
    };
    let of_git = |args: &[&str]| {
        git_output(path, args)
            .lines()
            .skip_while(|line| !line.starts_with("@@"))
            .filter(|line| !line.starts_with("@@"))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        patch_lines(&status.staged[0]),
        of_git(&["diff", "--cached"])
    );
    assert_eq!(patch_lines(&status.unstaged[0]), of_git(&["diff"]));

    let new = repo.diff(&status.untracked[0]).expect("diff");
    assert_eq!(new.before, None);
    assert_eq!(new.after.as_deref(), Some("note one\nnote two\n"));
    assert_eq!(new.file.lines.map(|l| l.added), Some(2));
}

#[test]
fn a_changed_ending_is_not_a_change_where_git_converts_it() {
    let dir = repo();
    let path = dir.path();
    git(path, &["config", "core.autocrlf", "true"]);
    // What a checkout on Windows leaves: the same lines, ending in `\r\n`.
    write(path, "a.txt", "one\r\ntwo\r\nthree\r\n");
    assert_agrees_with_git(path);
    write(path, "a.txt", "one\r\nTWO\r\nthree\r\n");
    assert_agrees_with_git(path);

    let repo = Repository::open(path).unwrap();
    let status = repo.status().unwrap();
    let diff = repo.diff(&status.unstaged[0]).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "one line changed, not every line");
    assert_eq!(diff.after.as_deref(), Some("one\nTWO\nthree\n"));
}

/// Git leaves alone a file the index already holds with `\\r\\n` in it, and
/// converts what it would otherwise: every line of it would read as changed.
#[test]
fn a_file_the_index_holds_with_carriage_returns_is_not_converted() {
    let dir = repo();
    let path = dir.path();
    git(path, &["config", "core.autocrlf", "false"]);
    write(path, "crlf.txt", "one\r\ntwo\r\nthree\r\n");
    git(path, &["add", "crlf.txt"]);
    git(path, &["commit", "--quiet", "-m", "lines ending in CRLF"]);
    git(path, &["config", "core.autocrlf", "true"]);
    write(path, "crlf.txt", "one\r\nTWO\r\nthree\r\n");
    assert_agrees_with_git(path);

    let repo = Repository::open(path).unwrap();
    let status = repo.status().unwrap();
    let diff = repo.diff(&status.unstaged[0]).expect("diff");
    assert_eq!(diff.hunks.len(), 1, "one line changed, not every line");
    let lines = diff.file.lines.expect("lines");
    assert_eq!((lines.added, lines.removed), (1, 1));
}

#[test]
fn a_merge_left_unresolved_is_one_row_of_its_own() {
    let dir = repo();
    let path = dir.path();
    git(path, &["checkout", "--quiet", "-b", "side"]);
    write(path, "a.txt", "side\n");
    git(path, &["commit", "--quiet", "-am", "side"]);
    git(path, &["checkout", "--quiet", "main"]);
    write(path, "a.txt", "main\n");
    git(path, &["commit", "--quiet", "-am", "main"]);
    // Exits non-zero, which is the point: it stops with the conflict.
    let merge = Command::new("git")
        .args(["merge", "--quiet", "side"])
        .current_dir(path)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_COMMITTER_NAME", "Ada Author")
        .env("GIT_COMMITTER_EMAIL", "ada@example.com")
        .output()
        .expect("git runs");
    assert!(!merge.status.success());

    assert_agrees_with_git(path);
    let status = Repository::open(path).unwrap().status().unwrap();
    assert_eq!(status.conflicted.len(), 1);
    assert_eq!(status.conflicted[0].path, "a.txt");
    assert_eq!(status.conflicted[0].kind, ConflictKind::BothModified);
    assert!(status.staged.is_empty(), "{:?}", status.staged);
}

#[test]
fn intent_to_add_is_not_yet_staged() {
    let dir = repo();
    write(dir.path(), "later.txt", "later\n");
    git(dir.path(), &["add", "--intent-to-add", "later.txt"]);
    assert_agrees_with_git(dir.path());
}

#[test]
fn a_repository_without_commits_has_its_index_staged() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path();
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    write(path, "first.txt", "first\n");
    write(path, "second.txt", "second\n");
    git(path, &["add", "first.txt"]);
    assert_agrees_with_git(path);
    let status = read_all(path);
    assert_eq!(status.staged[0].path, "first.txt");
    assert_eq!(status.untracked[0].path, "second.txt");
}

#[test]
fn untracked_files_stay_hidden_when_the_repository_says_so() {
    let dir = repo();
    let path = dir.path();
    git(path, &["config", "status.showUntrackedFiles", "no"]);
    write(path, "notes.txt", "notes\n");
    write(path, "a.txt", "changed\n");
    let status = Repository::open(path).unwrap().status().unwrap();
    assert_eq!(status.unstaged.len(), 1);
    assert!(status.untracked.is_empty());
}

#[test]
fn a_bare_repository_has_no_working_copy() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path();
    let output = Command::new("git")
        .args(["init", "--quiet", "--bare"])
        .current_dir(path)
        .output()
        .expect("git runs");
    assert!(output.status.success());
    assert!(Repository::open(path).unwrap().status().unwrap().is_clean());
}

/// A file that cannot be read stays in the list, and the rest are read.
#[test]
fn a_file_gone_since_it_was_listed_does_not_stop_the_others() {
    let dir = repo();
    let path = dir.path();
    write(path, "gone.txt", "soon gone\n");
    write(path, "stays.txt", "stays\n");
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    std::fs::remove_file(path.join("gone.txt")).expect("remove");

    while cache.read_files(&repo, 16) > 0 {}
    assert!(!cache.has_unread(), "the pass over the list ended");
    let row = |name: &str| {
        cache
            .status()
            .untracked
            .iter()
            .find(|file| file.path == name)
            .and_then(|file| file.after.as_ref())
            .map(|side| side.content.clone())
    };
    assert_eq!(row("gone.txt"), Some(Content::Unread));
    assert!(matches!(row("stays.txt"), Some(Content::Text { .. })));
}

#[test]
fn a_refresh_only_looks_at_what_it_is_told() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();

    write(path, "a.txt", "changed\n");
    write(path, "b.txt", "changed too, and nobody said so\n");
    let refreshed = cache.refresh(&repo, &["a.txt".to_owned()]).unwrap();
    assert_eq!(refreshed, Refreshed::Files(vec!["a.txt".to_owned()]));
    assert_eq!(cache.status().unstaged.len(), 1, "b.txt was not looked at");

    // Back to what is committed: the row goes, and that is a change too.
    write(path, "a.txt", "one\ntwo\nthree\n");
    let refreshed = cache.refresh(&repo, &["a.txt".to_owned()]).unwrap();
    assert_eq!(refreshed, Refreshed::Files(vec!["a.txt".to_owned()]));
    assert!(cache.status().unstaged.is_empty());
}

#[test]
fn what_git_ignores_moves_nothing() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();

    write(path, "target/debug/out.o", "built\n");
    write(path, "run.log", "log\n");
    let refreshed = cache
        .refresh(
            &repo,
            &[
                "target/debug/out.o".to_owned(),
                "run.log".to_owned(),
                "target".to_owned(),
            ],
        )
        .unwrap();
    assert_eq!(refreshed, Refreshed::Nothing);
    assert!(cache.status().is_clean());
}

/// A build writes thousands of files into an ignored folder. Named or not,
/// they are not worth a whole read: past `MOST_PATHS` of them that is what the
/// refresh falls back on, and it must not get that far.
#[test]
fn a_build_in_an_ignored_folder_costs_no_whole_read() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    let written: Vec<String> = (0..2_000)
        .map(|n| format!("target/debug/deps/out-{n}.o"))
        .collect();
    assert_eq!(cache.refresh(&repo, &written).unwrap(), Refreshed::Nothing);
}

/// The other side of that limit: that many paths git does not ignore are read
/// as a whole, and come to the same list.
#[test]
fn past_a_few_hundred_paths_a_whole_read_stands_in() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    let written: Vec<String> = (0..600).map(|n| format!("new/{n}.txt")).collect();
    for file in &written {
        write(path, file, "new\n");
    }
    assert_eq!(
        cache.refresh(&repo, &written).unwrap(),
        Refreshed::Everything
    );
    assert_eq!(cache.status().untracked.len(), 600);
    assert_eq!(rows_of(cache.status()), rows_of_git(path));
}

#[test]
fn a_file_tracked_in_an_ignored_folder_still_counts() {
    let dir = repo();
    let path = dir.path();
    write(path, "target/kept.txt", "kept\n");
    git(path, &["add", "--force", "target/kept.txt"]);
    git(path, &["commit", "--quiet", "-m", "keep one"]);
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();

    write(path, "target/kept.txt", "changed\n");
    let refreshed = cache
        .refresh(&repo, &["target/kept.txt".to_owned()])
        .unwrap();
    assert_eq!(
        refreshed,
        Refreshed::Files(vec!["target/kept.txt".to_owned()])
    );
    assert_agrees_with_git(path);
}

#[test]
fn a_path_with_odd_characters_is_taken_as_written() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();

    // Not a pattern: read as one, `[1]` is a character class and takes in
    // `a1.txt` as well, which nobody named.
    write(path, "a[1].txt", "new\n");
    write(path, "a1.txt", "new, and not named\n");
    write(path, "b.txt", "changed, and not named\n");
    let refreshed = cache.refresh(&repo, &["a[1].txt".to_owned()]).unwrap();
    assert_eq!(refreshed, Refreshed::Files(vec!["a[1].txt".to_owned()]));
    assert_eq!(cache.status().untracked.len(), 1);
    assert_eq!(cache.status().untracked[0].path, "a[1].txt");
    assert!(cache.status().unstaged.is_empty());
}

#[test]
fn a_folder_stands_for_everything_under_it() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();

    // A folder moved in is reported as the folder alone, and so is one gone.
    write(path, "moved/one.txt", "one\n");
    write(path, "moved/in/two.txt", "two\n");
    cache.refresh(&repo, &["moved".to_owned()]).unwrap();
    std::fs::remove_dir_all(path.join("src")).expect("remove");
    cache.refresh(&repo, &["src".to_owned()]).unwrap();
    assert_eq!(rows_of(cache.status()), rows_of_git(path));

    std::fs::remove_dir_all(path.join("moved")).expect("remove");
    let refreshed = cache.refresh(&repo, &["moved".to_owned()]).unwrap();
    assert_eq!(
        refreshed,
        Refreshed::Files(vec![
            "moved/in/two.txt".to_owned(),
            "moved/one.txt".to_owned()
        ])
    );
    assert_eq!(rows_of(cache.status()), rows_of_git(path));
}

#[test]
fn a_new_rule_reads_everything_again() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    write(path, "notes.txt", "notes\n");
    let mut cache = StatusCache::read(&repo).unwrap();
    assert_eq!(cache.status().untracked.len(), 1);

    write(path, ".gitignore", "target/\n*.log\nnotes.txt\n");
    let refreshed = cache.refresh(&repo, &[".gitignore".to_owned()]).unwrap();
    assert_eq!(refreshed, Refreshed::Everything);
    assert!(cache.status().untracked.is_empty());
    assert_eq!(rows_of(cache.status()), rows_of_git(path));
}

#[test]
fn a_write_of_the_index_reads_everything_again() {
    let dir = repo();
    let path = dir.path();
    let repo = Repository::open(path).unwrap();
    write(path, "a.txt", "changed\n");
    let mut cache = StatusCache::read(&repo).unwrap();
    assert_eq!(cache.status().unstaged.len(), 1);
    assert!(cache.status().staged.is_empty());

    git(path, &["add", "a.txt"]);
    let refreshed = cache.refresh(&repo, &[]).unwrap();
    assert_eq!(refreshed, Refreshed::Everything);
    assert_eq!(cache.status().staged.len(), 1);
    assert!(cache.status().unstaged.is_empty());

    git(path, &["commit", "--quiet", "-m", "second"]);
    assert_eq!(cache.refresh(&repo, &[]).unwrap(), Refreshed::Everything);
    assert!(cache.status().is_clean());
    assert_eq!(cache.refresh(&repo, &[]).unwrap(), Refreshed::Nothing);
}

/// What a staged file holds does not change, so it is not read twice.
#[test]
fn reading_everything_again_keeps_what_was_read_of_a_staged_file() {
    let dir = repo();
    let path = dir.path();
    write(path, "a.txt", "changed\n");
    git(path, &["add", "a.txt"]);
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    while cache.read_files(&repo, 16) > 0 {}
    assert!(cache.status().staged[0].lines.is_some());

    cache.read_again(&repo).unwrap();
    assert!(cache.status().staged[0].lines.is_some());
    assert!(!cache.has_unread());
}

#[test]
fn a_refreshed_file_is_read_again_and_the_others_are_kept() {
    let dir = repo();
    let path = dir.path();
    write(path, "a.txt", "one\nTWO\nthree\n");
    write(path, "b.txt", "BEE\n");
    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    while cache.read_files(&repo, 16) > 0 {}

    write(path, "a.txt", "ONE\nTWO\nTHREE\n");
    cache.refresh(&repo, &["a.txt".to_owned()]).unwrap();
    let find = |cache: &StatusCache, name: &str| {
        cache
            .status()
            .unstaged
            .iter()
            .find(|file| file.path == name)
            .cloned()
            .expect("a row")
    };
    assert_eq!(find(&cache, "a.txt").lines, None, "not read yet");
    assert!(find(&cache, "b.txt").lines.is_some(), "still as read");

    assert_eq!(cache.read_files(&repo, 16), 1, "the one file");
    assert_eq!(find(&cache, "a.txt").lines.map(|l| l.added), Some(3));
}

/// A tiny deterministic generator: a failure names the seed that made it.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

/// The property the whole cache rests on: told the paths that changed, a
/// refresh comes to what a whole read comes to, and to what git says - whatever
/// the sequence of writes, deletions, moves, stages and commits.
#[test]
fn a_refresh_comes_to_what_a_whole_read_comes_to() {
    const NAMES: [&str; 8] = [
        "a.txt",
        "b.txt",
        "src/lib.rs",
        "src/main.rs",
        "src/deep/er.rs",
        "new/one.txt",
        "new/two.txt",
        "target/ignored.o",
    ];
    for seed in [
        0x2545_F491_4F6C_DD1D_u64,
        0x9E37_79B9_7F4A_7C15,
        0xD120_5A3C_11E7_0B47,
    ] {
        let dir = repo();
        let path = dir.path();
        let repo = Repository::open(path).unwrap();
        let mut cache = StatusCache::read(&repo).unwrap();
        let mut rng = Rng(seed);
        for step in 0..60 {
            let name = NAMES[rng.below(NAMES.len() as u64) as usize];
            let mut said = vec![name.to_owned()];
            match rng.below(7) {
                0..=2 => write(path, name, &format!("step {step}\n")),
                3 => {
                    let _ = std::fs::remove_file(path.join(name));
                }
                4 => {
                    // A rename is two paths.
                    let to = NAMES[rng.below(NAMES.len() as u64) as usize];
                    if path.join(name).exists() {
                        if let Some(parent) = path.join(to).parent() {
                            std::fs::create_dir_all(parent).expect("folders");
                        }
                        std::fs::rename(path.join(name), path.join(to)).expect("move");
                    }
                    said.push(to.to_owned());
                }
                5 => git(path, &["add", "-A"]),
                _ => {
                    git(path, &["add", "-A"]);
                    git(path, &["commit", "--quiet", "--allow-empty", "-m", "step"]);
                }
            }
            cache.refresh(&repo, &said).expect("refresh");
            if step % 5 == 0 {
                while cache.read_files(&repo, 4) > 0 {}
            }
            let whole = repo.status().expect("read the status");
            assert_eq!(
                rows_of(cache.status()),
                rows_of(&whole),
                "seed {seed:#x}, step {step}, after {said:?}"
            );
            assert_eq!(
                rows_of(&whole),
                rows_of_git(path),
                "seed {seed:#x}, step {step}, after {said:?}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn a_mode_change_is_a_change_and_says_so() {
    use std::os::unix::fs::PermissionsExt;
    let dir = repo();
    let path = dir.path();
    let file = path.join("a.txt");
    let mut permissions = std::fs::metadata(&file).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&file, permissions).unwrap();
    assert_agrees_with_git(path);
    let status = Repository::open(path).unwrap().status().unwrap();
    assert_eq!(
        status.unstaged[0].after.as_ref().unwrap().mode,
        FileMode::Executable
    );
}

#[test]
fn a_submodule_with_changes_is_one_row() {
    let outer = repo();
    let inner = repo();
    let path = outer.path();
    git(
        path,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "--quiet",
            &inner.path().to_string_lossy(),
            "sub",
        ],
    );
    git(path, &["commit", "--quiet", "-m", "add a submodule"]);
    assert_agrees_with_git(path);

    let repo = Repository::open(path).unwrap();
    let mut cache = StatusCache::read(&repo).unwrap();
    assert!(cache.status().is_clean());

    write(path, "sub/a.txt", "changed in the submodule\n");
    let refreshed = cache.refresh(&repo, &["sub/a.txt".to_owned()]).unwrap();
    assert_eq!(refreshed, Refreshed::Files(vec!["sub".to_owned()]));
    assert_eq!(rows_of(cache.status()), rows_of_git(path));

    while cache.read_files(&repo, 16) > 0 {}
    let row = &cache.status().unstaged[0];
    assert_eq!(row.after.as_ref().unwrap().content, Content::Submodule);
    assert_eq!(row.after.as_ref().unwrap().mode, FileMode::Submodule);
}

#[cfg(feature = "watch")]
mod watch {
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    use furca_core::{Batch, Repository};

    use super::{git, repo, write};

    /// A file system takes its time; this is the most a test waits for it.
    const PATIENCE: Duration = Duration::from_secs(15);

    fn wait_for(batches: &Receiver<Batch>, what: impl Fn(&Batch) -> bool) -> Batch {
        let start = Instant::now();
        loop {
            let left = PATIENCE
                .checked_sub(start.elapsed())
                .expect("the batch never came");
            let batch = batches.recv_timeout(left).expect("the batch never came");
            if what(&batch) {
                return batch;
            }
        }
    }

    #[test]
    fn a_save_a_stage_and_a_commit_are_each_heard_for_what_they_are() {
        let dir = repo();
        let path = dir.path();
        let repo = Repository::open(path).unwrap();
        let (sender, batches) = mpsc::channel();
        let watcher = repo
            .watch(move |batch| {
                let _ = sender.send(batch);
            })
            .expect("watch");

        write(path, "src/lib.rs", "pub fn saved() {}\n");
        wait_for(&batches, |batch| {
            batch.paths.iter().any(|path| path == "src/lib.rs")
        });

        git(path, &["add", "src/lib.rs"]);
        wait_for(&batches, |batch| batch.index);

        git(path, &["commit", "--quiet", "-m", "second"]);
        wait_for(&batches, |batch| batch.refs);

        // What is in `.git` is never named as a path of the working tree.
        write(path, "c.txt", "new\n");
        let batch = wait_for(&batches, |batch| {
            batch.paths.iter().any(|path| path == "c.txt")
        });
        assert!(
            batch.paths.iter().all(|path| !path.starts_with(".git/")),
            "{:?}",
            batch.paths
        );

        // Dropped, it stops: the sender it held goes with it.
        drop(watcher);
        let start = Instant::now();
        while batches.recv_timeout(PATIENCE).is_ok() {
            assert!(start.elapsed() < PATIENCE, "the watcher never stopped");
        }
    }
}
