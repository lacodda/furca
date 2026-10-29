//! What a commit changed, against repositories built by the system `git` in
//! a temp directory: the files, what each side holds, and the lines - held
//! to what git itself says about the same commits.

use std::path::Path;
use std::process::Command;

use furca_core::{Content, Endings, FileChange, FileMode, FileStatus, LineKind, Repository};

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

    /// Runs `git` with no machine configuration: no autocrlf turning the
    /// test's line endings into the machine's, no hooks, no signing.
    fn git(&self, args: &[&str]) -> String {
        let empty = self.path().join(".git").join("no-config");
        if self.path().join(".git").exists() {
            std::fs::write(&empty, "").expect("empty config");
        }
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Author")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Author")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", "1700000000 +0000")
            .env("GIT_COMMITTER_DATE", "1700000000 +0000")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    }

    fn write(&self, path: &str, content: impl AsRef<[u8]>) {
        let path = self.path().join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("dirs");
        std::fs::write(path, content).expect("write");
    }

    /// Stages everything and commits it; returns the commit's id.
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--allow-empty", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn open(&self) -> Repository {
        Repository::open(self.path()).expect("furca-core opens the fixture")
    }
}

fn file<'a>(files: &'a [FileChange], path: &str) -> &'a FileChange {
    files
        .iter()
        .find(|file| file.path == path)
        .unwrap_or_else(|| panic!("{path} is among the changes: {files:#?}"))
}

/// `count` numbered lines, `\n`-terminated.
fn lines(count: u32, word: &str) -> String {
    (1..=count).map(|n| format!("{word} {n}\n")).collect()
}

#[test]
fn a_commit_lists_its_files_with_how_each_changed_and_its_lines() {
    let fixture = Fixture::new();
    fixture.write("keep.txt", lines(10, "line"));
    fixture.write("gone.txt", "bye\n");
    let root = fixture.commit("first");
    let mut kept = lines(10, "line");
    kept = kept
        .replace("line 3\n", "line three\n")
        .replace("line 7\n", "");
    kept.push_str("line 11\nline 12\n");
    fixture.write("keep.txt", &kept);
    std::fs::remove_file(fixture.path().join("gone.txt")).expect("remove");
    fixture.write("new/born.txt", "hello\nworld\n");
    let second = fixture.commit("second");

    let changes = fixture.open().changes("HEAD").expect("changes");
    assert_eq!(changes.commit, second);
    assert_eq!(changes.parent.as_deref(), Some(root.as_str()));
    let paths: Vec<&str> = changes.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        ["gone.txt", "keep.txt", "new/born.txt"],
        "sorted by path"
    );

    let keep = file(&changes.files, "keep.txt");
    assert_eq!(keep.status, FileStatus::Modified);
    let lines = keep.lines.expect("text has lines");
    assert_eq!((lines.added, lines.removed), (3, 2));
    let after = keep.after.as_ref().expect("an after side");
    assert_eq!(
        after.content,
        Content::Text {
            endings: Endings::Lf
        }
    );
    assert_eq!(after.size, Some(kept.len() as u64));
    assert_eq!(after.mode, FileMode::File);

    let gone = file(&changes.files, "gone.txt");
    assert_eq!(gone.status, FileStatus::Deleted);
    assert!(gone.after.is_none());
    assert_eq!(gone.lines.map(|l| (l.added, l.removed)), Some((0, 1)));

    let born = file(&changes.files, "new/born.txt");
    assert_eq!(born.status, FileStatus::Added);
    assert!(born.before.is_none());
    assert_eq!(born.lines.map(|l| (l.added, l.removed)), Some((2, 0)));
}

#[test]
fn the_counts_are_the_ones_git_numstat_gives() {
    let fixture = Fixture::new();
    let mut text = lines(300, "row");
    fixture.write("a.rs", &text);
    fixture.commit("first");
    // Moves, insertions and a rewritten block: where two diffs could
    // disagree about which lines changed.
    text = text.replace("row 20\n", "row twenty\nrow twenty-one\n");
    text = text.replace("row 150\nrow 151\nrow 152\n", "");
    text.push_str("row 1\nrow 2\n");
    text = text.replacen("row 1\n", "row zero\nrow 1\n", 1);
    fixture.write("a.rs", &text);
    fixture.commit("second");

    let numstat = fixture.git(&["diff", "--numstat", "HEAD~1", "HEAD"]);
    let (added, removed) = {
        let mut fields = numstat.split_whitespace();
        let added: u32 = fields.next().expect("added").parse().expect("a number");
        let removed: u32 = fields.next().expect("removed").parse().expect("a number");
        (added, removed)
    };
    let changes = fixture.open().changes("HEAD").expect("changes");
    let lines = file(&changes.files, "a.rs").lines.expect("lines");
    assert_eq!((lines.added, lines.removed), (added, removed));
}

#[test]
fn a_patch_reads_line_for_line_as_git_diff_writes_it() {
    let fixture = Fixture::new();
    let mut text = lines(80, "item");
    fixture.write("list.txt", &text);
    fixture.commit("first");
    text = text
        .replace("item 2\n", "item two\n")
        .replace("item 40\n", "item forty\nitem forty and a half\n")
        .replace("item 44\n", "")
        // Five unchanged lines apart: git joins them into one hunk, since
        // each one's context would touch the other's.
        .replace("item 60\n", "item sixty\n")
        .replace("item 66\n", "item sixty-six\n")
        .replace("item 80\n", "item eighty\n");
    fixture.write("list.txt", &text);
    fixture.commit("second");

    let repo = fixture.open();
    let changes = repo.changes("HEAD").expect("changes");
    let diff = repo.diff(file(&changes.files, "list.txt")).expect("diff");
    let ours: Vec<String> = diff
        .patch(3)
        .hunks
        .iter()
        .flat_map(|hunk| {
            let header = |start: u32, count: u32| {
                let first = if count == 0 { start } else { start + 1 };
                if count == 1 {
                    first.to_string()
                } else {
                    format!("{first},{count}")
                }
            };
            std::iter::once(format!(
                "@@ -{} +{} @@",
                header(hunk.before.start, hunk.before.count),
                header(hunk.after.start, hunk.after.count)
            ))
            .chain(hunk.lines.iter().map(|line| {
                let mark = match line.kind {
                    LineKind::Context => ' ',
                    LineKind::Removed => '-',
                    LineKind::Added => '+',
                };
                format!("{mark}{}", line.text)
            }))
        })
        .collect();
    let git = fixture.git(&["diff", "-U3", "--no-color", "HEAD~1", "HEAD"]);
    let theirs: Vec<String> = git
        .lines()
        .skip_while(|line| !line.starts_with("@@"))
        // git puts the function it guessed after the second `@@`.
        .map(|line| match line.strip_prefix("@@") {
            Some(rest) => format!("@@{}@@", rest.split("@@").next().unwrap_or_default()),
            None => line.to_owned(),
        })
        .collect();
    assert_eq!(ours, theirs);
}

#[test]
fn a_rename_names_where_the_file_came_from_and_how_alike_it_stayed() {
    let fixture = Fixture::new();
    fixture.write("docs/old.md", lines(40, "para"));
    fixture.write("same.txt", lines(20, "word"));
    // Half alike by git's count, where rounding and git part ways.
    fixture.write(
        "intro.md",
        "# Introduction

This guide shows the library.

It has two functions.
",
    );
    fixture.commit("first");
    std::fs::remove_file(fixture.path().join("intro.md")).expect("remove");
    fixture.write(
        "guide.md",
        "# Introduction

This guide shows the library.

It has two functions, and greet takes a name.
",
    );
    std::fs::remove_file(fixture.path().join("docs/old.md")).expect("remove");
    fixture.write(
        "docs/new.md",
        lines(40, "para").replace("para 5\n", "para five\n"),
    );
    std::fs::rename(
        fixture.path().join("same.txt"),
        fixture.path().join("moved.txt"),
    )
    .expect("move");
    fixture.commit("move");

    let changes = fixture.open().changes("HEAD").expect("changes");
    assert_eq!(
        changes.files.len(),
        3,
        "three renames, not deletions and additions"
    );
    let renamed = file(&changes.files, "docs/new.md");
    assert_eq!(renamed.status, FileStatus::Renamed);
    assert_eq!(renamed.from.as_deref(), Some("docs/old.md"));
    let similarity = renamed.similarity.expect("a similarity");
    assert!((90..100).contains(&similarity), "{similarity}");
    let summary = fixture.git(&["diff", "-M", "--summary", "HEAD~1", "HEAD"]);
    for path in ["docs/new.md", "guide.md"] {
        let name = path.rsplit('/').next().unwrap_or(path);
        let git_says = summary
            .lines()
            .find(|line| line.contains(name))
            .and_then(|line| {
                line.rsplit_once('(')?
                    .1
                    .strip_suffix("%)")?
                    .parse::<u8>()
                    .ok()
            })
            .unwrap_or_else(|| panic!("git names the rename of {path}: {summary}"));
        assert_eq!(
            file(&changes.files, path).similarity,
            Some(git_says),
            "{path}: the figure git gives"
        );
    }
    assert_eq!(renamed.lines.map(|l| (l.added, l.removed)), Some((1, 1)));

    let moved = file(&changes.files, "moved.txt");
    assert_eq!(moved.from.as_deref(), Some("same.txt"));
    assert_eq!(moved.similarity, Some(100));
    assert_eq!(moved.lines.map(|l| (l.added, l.removed)), Some((0, 0)));
}

#[test]
fn a_binary_says_so_with_its_size_and_has_no_lines() {
    let fixture = Fixture::new();
    let mut image = vec![0x89, b'P', b'N', b'G', 0, 0, 0, 13];
    image.extend((0..4000u32).map(|n| (n % 251) as u8));
    fixture.write("logo.png", &image);
    // Text by its bytes, binary by the attribute: the attribute decides.
    fixture.write(".gitattributes", "*.dat binary\n");
    fixture.write("table.dat", "a,b\n1,2\n");
    fixture.commit("add");

    let changes = fixture.open().changes("HEAD").expect("changes");
    for (path, size) in [("logo.png", image.len()), ("table.dat", 8)] {
        let change = file(&changes.files, path);
        let after = change.after.as_ref().expect("after");
        assert_eq!(after.content, Content::Binary, "{path}");
        assert_eq!(after.size, Some(size as u64), "{path}");
        assert_eq!(change.lines, None, "{path} has no lines to count");
    }
}

#[test]
fn a_git_lfs_pointer_names_the_file_it_stands_for() {
    let fixture = Fixture::new();
    let oid = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
    fixture.write(
        "video.mp4",
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 126418944\n"),
    );
    // Says it is a pointer and is not one: an object id that is not hex.
    fixture.write(
        "notes.txt",
        "version https://git-lfs.github.com/spec/v1\noid sha256:not-a-hash\nsize 12\n",
    );
    fixture.commit("add");

    let changes = fixture.open().changes("HEAD").expect("changes");
    let video = file(&changes.files, "video.mp4");
    let after = video.after.as_ref().expect("after");
    assert_eq!(
        after.content,
        Content::Lfs {
            oid: oid.to_owned()
        }
    );
    assert_eq!(
        after.size,
        Some(126_418_944),
        "the size of the file, not of the pointer"
    );
    assert_eq!(video.lines, None);
    let notes = file(&changes.files, "notes.txt");
    assert!(matches!(
        notes.after.as_ref().expect("after").content,
        Content::Text { .. }
    ));
}

#[test]
fn a_line_that_only_changed_its_ending_is_a_changed_line() {
    let fixture = Fixture::new();
    fixture.write("win.txt", "one\r\ntwo\r\nthree\r\n");
    fixture.write("mixed.txt", "one\r\ntwo\n");
    fixture.commit("first");
    fixture.write("win.txt", "one\ntwo\nthree\n");
    fixture.write("mixed.txt", "one\r\ntwo\nthree");
    fixture.commit("second");

    let changes = fixture.open().changes("HEAD").expect("changes");
    let win = file(&changes.files, "win.txt");
    assert_eq!(win.lines.map(|l| (l.added, l.removed)), Some((3, 3)));
    let ending = |side: &Option<furca_core::Side>| match &side.as_ref().expect("a side").content {
        Content::Text { endings } => *endings,
        other => panic!("text, not {other:?}"),
    };
    assert_eq!(ending(&win.before), Endings::Crlf);
    assert_eq!(ending(&win.after), Endings::Lf);
    assert_eq!(
        ending(&file(&changes.files, "mixed.txt").after),
        Endings::Mixed
    );
}

#[test]
fn a_submodule_is_a_commit_not_a_file() {
    let fixture = Fixture::new();
    let pinned = "1234567890123456789012345678901234567890";
    fixture.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{pinned},vendor/lib"),
    ]);
    fixture.git(&["commit", "--quiet", "-m", "pin a submodule"]);

    let changes = fixture.open().changes("HEAD").expect("changes");
    let module = file(&changes.files, "vendor/lib");
    let after = module.after.as_ref().expect("after");
    assert_eq!(after.mode, FileMode::Submodule);
    assert_eq!(after.content, Content::Submodule);
    assert_eq!(after.id, pinned);
    assert_eq!(after.size, None);
    assert_eq!(module.lines, None);
}

#[test]
fn a_root_commit_adds_everything_and_a_merge_is_held_to_its_first_parent() {
    let fixture = Fixture::new();
    fixture.write("a.txt", "a\n");
    let root = fixture.commit("root");
    let changes = fixture.open().changes(&root).expect("changes");
    assert_eq!(changes.parent, None);
    assert_eq!(file(&changes.files, "a.txt").status, FileStatus::Added);

    fixture.git(&["checkout", "--quiet", "-b", "side"]);
    fixture.write("side.txt", "from the side\n");
    fixture.commit("side");
    fixture.git(&["checkout", "--quiet", "main"]);
    fixture.write("main.txt", "on main\n");
    let main = fixture.commit("main");
    fixture.git(&["merge", "--quiet", "--no-ff", "-m", "merge side", "side"]);

    let merge = fixture.open().changes("HEAD").expect("changes");
    assert_eq!(merge.parent.as_deref(), Some(main.as_str()));
    let paths: Vec<&str> = merge.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["side.txt"], "what the merge brought into main");
}

#[test]
fn a_diff_carries_both_texts_whole_and_the_hunks_between_them() {
    let fixture = Fixture::new();
    fixture.write("poem.txt", "a\nb\nc\nd\n");
    fixture.write("img.bin", [0u8, 1, 2, 3]);
    fixture.commit("first");
    fixture.write("poem.txt", "a\nB\nc\nd\ne");
    fixture.write("img.bin", [0u8, 1, 2, 3, 4]);
    fixture.commit("second");

    let repo = fixture.open();
    let changes = repo.changes("HEAD").expect("changes");
    let diff = repo.diff(file(&changes.files, "poem.txt")).expect("diff");
    assert_eq!(diff.before.as_deref(), Some("a\nb\nc\nd\n"));
    assert_eq!(diff.after.as_deref(), Some("a\nB\nc\nd\ne"));
    let hunks: Vec<((u32, u32), (u32, u32))> = diff
        .hunks
        .iter()
        .map(|h| {
            (
                (h.before.start, h.before.count),
                (h.after.start, h.after.count),
            )
        })
        .collect();
    assert_eq!(hunks, [((1, 1), (1, 1)), ((4, 0), (4, 1))]);

    let binary = repo.diff(file(&changes.files, "img.bin")).expect("diff");
    assert_eq!((binary.before, binary.after), (None, None));
    assert!(binary.hunks.is_empty());
    assert_eq!(binary.file.after.as_ref().and_then(|s| s.size), Some(5));
}

#[test]
fn a_file_is_read_at_a_revision_as_its_bytes() {
    let fixture = Fixture::new();
    fixture.write("notes/today.md", b"first\r\n\xff\n");
    let first = fixture.commit("first");
    fixture.write("notes/today.md", "second\n");
    fixture.commit("second");

    let repo = fixture.open();
    assert_eq!(
        repo.file_at(&first, "notes/today.md").expect("read"),
        Some(b"first\r\n\xff\n".to_vec()),
        "byte for byte, not as text"
    );
    assert_eq!(
        repo.file_at("HEAD", "notes/today.md").expect("read"),
        Some(b"second\n".to_vec())
    );
    assert_eq!(
        repo.file_at("HEAD", "notes/missing.md").expect("read"),
        None
    );
    assert_eq!(
        repo.file_at("HEAD", "notes").expect("read"),
        None,
        "a directory is no file"
    );
}

#[test]
fn a_file_history_follows_the_file_through_its_renames() {
    let fixture = Fixture::new();
    fixture.write("draft.md", lines(30, "thought"));
    let born = fixture.commit("born");
    fixture.write("other.txt", "unrelated\n");
    fixture.commit("elsewhere");
    fixture.write(
        "draft.md",
        lines(30, "thought").replace("thought 2\n", "second thought\n"),
    );
    let edited = fixture.commit("edited");
    std::fs::rename(
        fixture.path().join("draft.md"),
        fixture.path().join("essay.md"),
    )
    .expect("rename");
    let moved = fixture.commit("moved");
    fixture.write(
        "essay.md",
        lines(30, "thought").replace("thought 2\n", "second thought\n") + "coda\n",
    );
    let finished = fixture.commit("finished");

    let repo = fixture.open();
    let log = repo.file_log("essay.md", 10).expect("log");
    let seen: Vec<(&str, &str, FileStatus, Option<&str>)> = log
        .commits
        .iter()
        .map(|r| {
            (
                r.commit.id.as_str(),
                r.path.as_str(),
                r.status,
                r.from.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        [
            (finished.as_str(), "essay.md", FileStatus::Modified, None),
            (
                moved.as_str(),
                "essay.md",
                FileStatus::Renamed,
                Some("draft.md")
            ),
            (edited.as_str(), "draft.md", FileStatus::Modified, None),
            (born.as_str(), "draft.md", FileStatus::Added, None),
        ]
    );
    assert!(!log.truncated);
    // Each revision's path reads the file as it was there.
    assert!(
        repo.file_at(&edited, &log.commits[2].path)
            .expect("read")
            .is_some()
    );

    let cut = repo.file_log("essay.md", 2).expect("log");
    assert_eq!(cut.commits.len(), 2);
    assert!(cut.truncated);

    std::fs::remove_file(fixture.path().join("essay.md")).expect("remove");
    let gone = fixture.commit("gone");
    let repo = fixture.open();
    let after = repo.file_log("essay.md", 1).expect("log");
    let last = &after.commits[0];
    assert_eq!(
        (last.commit.id.as_str(), last.status),
        (gone.as_str(), FileStatus::Deleted)
    );
    assert!(
        repo.file_log("never-there.md", 10)
            .expect("log")
            .commits
            .is_empty()
    );
}

#[test]
fn details_carry_the_whole_message_with_the_changes() {
    let fixture = Fixture::new();
    fixture.write("a.txt", "a\n");
    fixture.git(&["add", "--all"]);
    fixture.git(&[
        "commit",
        "--quiet",
        "-m",
        "feat: a subject",
        "-m",
        "A body\nof two lines.",
    ]);

    let details = fixture.open().details("HEAD").expect("details");
    assert_eq!(details.commit.subject, "feat: a subject");
    assert_eq!(details.message, "feat: a subject\n\nA body\nof two lines.");
    assert_eq!(details.changes.commit, details.commit.id);
    assert_eq!(details.changes.files.len(), 1);
}
