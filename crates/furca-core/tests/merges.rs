//! Merging, picking and reverting, the operations they leave halfway, and
//! resolving what they left in conflict, against a throwaway repository in a
//! temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{
    BlockKind, ConflictKind, Error, FastForward, Operation, Pick, Proceed, Repository, Resolution,
};

struct Place {
    dir: tempfile::TempDir,
}

impl Place {
    /// A repository on `main` with `f.txt` of five lines, committed.
    fn new() -> Place {
        let place = Place {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        let root = place.dir.path();
        std::fs::create_dir_all(root.join("no-hooks")).expect("hooks dir");
        git(root, &["init", "--quiet", "--initial-branch=main", "work"]);
        configure(&place.work(), &root.join("no-hooks"));
        place.write("f.txt", "one\ntwo\nthree\nfour\nfive\n");
        place.commit("feat: start");
        place
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn open(&self) -> Repository {
        Repository::open(self.work()).expect("furca-core opens the repository")
    }

    fn write(&self, file: &str, text: &str) {
        std::fs::write(self.work().join(file), text).expect("write");
    }

    fn read(&self, file: &str) -> String {
        std::fs::read_to_string(self.work().join(file)).expect("read")
    }

    fn commit(&self, message: &str) -> String {
        git(&self.work(), &["add", "--all"]);
        git(&self.work(), &["commit", "--quiet", "-m", message]);
        self.rev("HEAD")
    }

    fn rev(&self, revision: &str) -> String {
        git(&self.work(), &["rev-parse", revision])
    }

    fn git(&self, args: &[&str]) -> String {
        git(&self.work(), args)
    }

    /// `feature` and `main` both rewrite the second line of `f.txt`, each its
    /// own way, and `HEAD` is back on `main`.
    fn parted(&self) -> (String, String) {
        self.git(&["switch", "--quiet", "--create", "feature"]);
        self.write("f.txt", "one\nTWO from feature\nthree\nfour\nfive\n");
        let feature = self.commit("feat: shout two");
        self.git(&["switch", "--quiet", "main"]);
        self.write("f.txt", "one\n2 from main\nthree\nfour\nFIVE\n");
        let main = self.commit("feat: number two");
        (main, feature)
    }
}

/// A repository's own configuration that shuts out the machine's, where the
/// core's own runs of git read it too.
fn configure(dir: &Path, hooks: &Path) {
    for (key, value) in [
        ("user.name", "Ada Author"),
        ("user.email", "ada@example.com"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.autocrlf", "false"),
        ("merge.ff", "true"),
        ("rerere.enabled", "false"),
        ("core.hooksPath", hooks.to_str().expect("utf-8 path")),
    ] {
        git(dir, &["config", key, value]);
    }
}

/// The test's own `git`, with the machine's configuration shut out.
fn git(dir: &Path, args: &[&str]) -> String {
    let output = run(dir, args);
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

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    let empty = std::env::temp_dir().join("furca-core-tests.gitconfig");
    if !empty.exists() {
        std::fs::write(&empty, "").expect("empty config");
    }
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &empty)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("git runs")
}

#[test]
fn a_branch_ahead_is_merged_by_moving_along_to_it() {
    let place = Place::new();
    let start = place.rev("HEAD");
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    let tip = place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);

    let stepped = place
        .open()
        .git()
        .merge("feature", FastForward::AsConfigured)
        .expect("merges");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.stopped, None);
    assert_eq!(stepped.from.as_deref(), Some(start.as_str()));
    assert_eq!(stepped.to.as_deref(), Some(tip.as_str()));
    assert_eq!(stepped.made, 1);
    assert_eq!(stepped.branch.as_deref(), Some("main"));
}

#[test]
fn a_merge_commit_names_what_it_merged_by_its_short_name() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);

    let stepped = place
        .open()
        .git()
        .merge("feature", FastForward::Never)
        .expect("merges");
    assert_eq!(stepped.made, 2, "the merge commit and the one it brought");
    assert_eq!(
        place.git(&["log", "-1", "--format=%s"]),
        "Merge branch 'feature'"
    );
    assert_eq!(
        place.git(&["log", "-1", "--format=%p"]).split(' ').count(),
        2
    );
}

#[test]
fn a_name_another_ref_shares_is_merged_by_its_full_name() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "same"]);
    place.write("g.txt", "g\n");
    let branch = place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    // A tag of the same name, on another commit: git would take it first.
    place.git(&["tag", "same", "HEAD"]);

    let repo = place.open();
    assert_eq!(
        repo.merge_target("same").expect("a target"),
        "refs/heads/same"
    );
    assert_eq!(
        repo.merge_target("refs/heads/same").expect("a target"),
        "refs/heads/same",
        "the branch, asked for by its full name"
    );
    assert_eq!(
        repo.merge_target("refs/tags/same").expect("a target"),
        "same",
        "the tag, asked for by its full name: git finds it first by its short one"
    );
    assert!(
        repo.merge_target("refs/heads/gone").is_err(),
        "no such branch, and no commit by that name either"
    );
    let stepped = repo
        .git()
        .merge("same", FastForward::AsConfigured)
        .expect("merges");
    assert_eq!(
        stepped.to.as_deref(),
        Some(branch.as_str()),
        "the branch, not the tag"
    );
}

#[test]
fn the_branch_head_is_on_is_not_merged_into_itself() {
    let place = Place::new();
    let refused = place.open().merge_target("main");
    assert!(matches!(refused, Err(Error::Operation(_))), "{refused:?}");
}

#[test]
fn a_merge_that_meets_a_conflict_stops_halfway_and_says_what_it_brings() {
    let place = Place::new();
    let (main, feature) = place.parted();

    let repo = place.open();
    let stepped = repo
        .git()
        .merge("feature", FastForward::AsConfigured)
        .expect("a conflict is an answer, not an error");
    assert_eq!(stepped.operation, Some(Operation::Merge));
    assert!(
        stepped
            .stopped
            .as_deref()
            .is_some_and(|said| said.contains("f.txt")),
        "{stepped:?}"
    );
    assert_eq!(stepped.conflicts.len(), 1);
    assert_eq!(stepped.conflicts[0].path, "f.txt");
    assert_eq!(stepped.conflicts[0].kind, ConflictKind::BothModified);
    assert_eq!(
        stepped.to.as_deref(),
        Some(main.as_str()),
        "nothing committed yet"
    );

    let halfway = repo.halfway().expect("reads").expect("halfway");
    assert_eq!(halfway.operation, Operation::Merge);
    assert_eq!(halfway.branch.as_deref(), Some("main"));
    assert_eq!(
        halfway.commit.as_ref().map(|c| c.id.as_str()),
        Some(feature.as_str())
    );
    assert_eq!(halfway.message.as_deref(), Some("Merge branch 'feature'"));
    assert_eq!(halfway.conflicts, 1);
    assert!(halfway.can_continue);
    assert!(!halfway.can_skip);

    let again = repo.git().merge("feature", FastForward::AsConfigured);
    assert!(
        matches!(
            again,
            Err(Error::Halfway {
                operation: Operation::Merge
            })
        ),
        "{again:?}"
    );
    let skipped = repo.git().proceed(Proceed::Skip);
    assert!(matches!(skipped, Err(Error::Operation(_))), "{skipped:?}");
    let early = repo.git().proceed(Proceed::Continue);
    assert!(
        matches!(early, Err(Error::Unresolved { paths: 1 })),
        "{early:?}"
    );
}

#[test]
fn a_conflict_is_read_from_the_stages_in_blocks_whatever_the_markers_say() {
    let place = Place::new();
    place.parted();
    // The markers git writes leave the ancestor out in this style: the
    // blocks do not depend on them.
    place.git(&["config", "merge.conflictStyle", "merge"]);
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");

    let conflict = repo.conflict("f.txt").expect("reads the conflict");
    assert_eq!(conflict.kind, ConflictKind::BothModified);
    let texts = conflict.texts.as_ref().expect("text on every side");
    assert_eq!(texts.base, "one\ntwo\nthree\nfour\nfive\n");
    assert_eq!(texts.ours, "one\n2 from main\nthree\nfour\nFIVE\n");
    assert_eq!(texts.theirs, "one\nTWO from feature\nthree\nfour\nfive\n");
    let kinds: Vec<BlockKind> = conflict.blocks.iter().map(|block| block.kind).collect();
    assert_eq!(
        kinds,
        [
            BlockKind::Unchanged,
            BlockKind::Conflict,
            BlockKind::Unchanged,
            BlockKind::Ours
        ]
    );
    assert_eq!(conflict.conflicts(), 1);
    assert_eq!(
        conflict.marked,
        Some(2),
        "git's markers, from the second line"
    );
    assert_eq!(
        conflict.merged(Pick::Theirs).as_deref(),
        Some("one\nTWO from feature\nthree\nfour\nFIVE\n"),
        "theirs in the conflict, and our change that did not conflict"
    );
    assert_eq!(repo.conflicts().expect("lists").len(), 1);
    let none = repo.conflict("nothing.txt");
    assert!(matches!(none, Err(Error::Operation(_))), "{none:?}");
}

#[test]
fn a_resolution_written_and_continued_makes_the_merge_commit() {
    let place = Place::new();
    let (main, feature) = place.parted();
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");

    let text = repo
        .conflict("f.txt")
        .expect("reads")
        .merged(Pick::Both)
        .expect("in blocks");
    repo.git()
        .resolve("f.txt", &Resolution::Text { text })
        .expect("resolves");
    assert_eq!(
        place.read("f.txt"),
        "one\n2 from main\nTWO from feature\nthree\nfour\nFIVE\n"
    );
    assert!(repo.conflicts().expect("lists").is_empty());
    assert_eq!(
        place.git(&["diff", "--name-only"]),
        "",
        "the file is staged as written"
    );

    let stepped = repo.git().proceed(Proceed::Continue).expect("continues");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.stopped, None);
    assert_eq!(stepped.made, 2);
    assert_eq!(
        place.git(&["log", "-1", "--format=%P"]),
        format!("{main} {feature}")
    );
    assert_eq!(
        place.git(&["log", "-1", "--format=%s"]),
        "Merge branch 'feature'"
    );
    assert!(repo.halfway().expect("reads").is_none());
}

#[test]
fn an_abort_puts_everything_back() {
    let place = Place::new();
    let (main, _) = place.parted();
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");

    let stepped = repo.git().proceed(Proceed::Abort).expect("aborts");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.to.as_deref(), Some(main.as_str()));
    assert_eq!(place.read("f.txt"), "one\n2 from main\nthree\nfour\nFIVE\n");
    assert!(repo.halfway().expect("reads").is_none());
    let nothing = repo.git().proceed(Proceed::Abort);
    assert!(matches!(nothing, Err(Error::Operation(_))), "{nothing:?}");
}

#[test]
fn a_file_taken_as_it_is_is_refused_while_it_holds_the_markers() {
    let place = Place::new();
    place.parted();
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");

    let refused = repo
        .git()
        .resolve("f.txt", &Resolution::AsIs { force: false });
    assert!(
        matches!(&refused, Err(Error::Markers { path, line: 2 }) if path == "f.txt"),
        "{refused:?}"
    );
    place.write("f.txt", "one\nedited by hand\nthree\nfour\nFIVE\n");
    repo.git()
        .resolve("f.txt", &Resolution::AsIs { force: false })
        .expect("resolves");
    assert!(repo.conflicts().expect("lists").is_empty());
    assert_eq!(
        place.git(&["show", ":f.txt"]),
        "one\nedited by hand\nthree\nfour\nFIVE"
    );
}

#[test]
fn a_whole_side_is_taken_with_what_did_not_conflict_left_behind() {
    let place = Place::new();
    place.parted();
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");
    repo.git()
        .resolve("f.txt", &Resolution::Theirs)
        .expect("resolves");
    assert_eq!(
        place.read("f.txt"),
        "one\nTWO from feature\nthree\nfour\nfive\n"
    );
    assert!(repo.conflicts().expect("lists").is_empty());
}

#[test]
fn a_file_one_side_deleted_is_kept_or_deleted_whole() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.git(&["rm", "--quiet", "f.txt"]);
    place.commit("refactor: drop f");
    place.git(&["switch", "--quiet", "main"]);
    place.write("f.txt", "one\nchanged on main\nthree\nfour\nfive\n");
    place.commit("feat: change f");

    let repo = place.open();
    let stepped = repo
        .git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");
    assert_eq!(stepped.conflicts[0].kind, ConflictKind::DeletedByThem);
    let conflict = repo.conflict("f.txt").expect("reads");
    assert!(conflict.theirs.is_none());
    assert!(conflict.blocks.is_empty(), "taken whole, not in blocks");
    assert_eq!(conflict.merged(Pick::Ours), None);

    repo.git()
        .resolve("f.txt", &Resolution::Theirs)
        .expect("resolves as deleted");
    assert!(!place.work().join("f.txt").exists());
    assert!(repo.conflicts().expect("lists").is_empty());
    repo.git().proceed(Proceed::Continue).expect("continues");
    assert_eq!(place.git(&["ls-files", "f.txt"]), "");
}

#[test]
fn a_pick_that_meets_a_conflict_can_be_skipped() {
    let place = Place::new();
    let (main, feature) = place.parted();
    let repo = place.open();

    let stepped = repo.git().cherry_pick(&[&feature]).expect("stops");
    assert_eq!(stepped.operation, Some(Operation::CherryPick));
    let halfway = repo.halfway().expect("reads").expect("halfway");
    assert!(halfway.can_skip);
    assert_eq!(
        halfway.commit.map(|c| c.subject),
        Some("feat: shout two".to_owned())
    );

    // Taking ours leaves the pick with nothing in it: git refuses to make
    // an empty commit, and the pick stays where it was.
    repo.git()
        .resolve("f.txt", &Resolution::Ours)
        .expect("resolves");
    let empty = repo.git().proceed(Proceed::Continue);
    assert!(matches!(empty, Err(Error::Git { .. })), "{empty:?}");
    assert!(repo.halfway().expect("reads").is_some());

    let skipped = repo.git().proceed(Proceed::Skip).expect("skips");
    assert_eq!(skipped.operation, None);
    assert_eq!(skipped.to.as_deref(), Some(main.as_str()));
}

#[test]
fn a_pick_makes_a_commit_with_the_commit_s_own_message() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    let tip = place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    place.write("h.txt", "h\n");
    place.commit("feat: add h");

    let repo = place.open();
    let stepped = repo.git().cherry_pick(&["feature"]).expect("picks");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.made, 1);
    assert_eq!(place.git(&["log", "-1", "--format=%s"]), "feat: add g");
    assert_ne!(place.rev("HEAD"), tip);

    let again = repo.git().cherry_pick(&["HEAD~1"]);
    assert!(matches!(again, Err(Error::Operation(_))), "{again:?}");
}

#[test]
fn a_merge_commit_is_reverted_against_the_line_it_was_merged_into() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::Never)
        .expect("merges");
    assert!(place.work().join("g.txt").exists());

    let stepped = place.open().git().revert(&["HEAD"]).expect("reverts");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.made, 1);
    assert!(
        !place.work().join("g.txt").exists(),
        "what the merge brought is gone"
    );
    assert!(
        place
            .git(&["log", "-1", "--format=%s"])
            .starts_with("Revert \"Merge branch 'feature'\"")
    );
}

#[test]
fn a_resolution_keeps_the_line_endings_the_file_had() {
    let place = Place::new();
    place.write("w.txt", "a\r\nb\r\nc\r\n");
    place.commit("feat: add w");
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("w.txt", "a\r\nB\r\nc\r\n");
    place.commit("feat: theirs");
    place.git(&["switch", "--quiet", "main"]);
    place.write("w.txt", "a\r\nb!\r\nc\r\n");
    place.commit("feat: ours");

    let repo = place.open();
    repo.git()
        .merge("feature", FastForward::AsConfigured)
        .expect("stops");
    repo.git()
        .resolve(
            "w.txt",
            &Resolution::Text {
                text: "a\nboth\nc\n".to_owned(),
            },
        )
        .expect("resolves");
    let staged = run(&place.work(), &["show", ":w.txt"]).stdout;
    assert_eq!(staged, b"a\r\nboth\r\nc\r\n");
}

#[test]
fn a_rebase_stopped_at_a_conflict_is_resolved_and_continued_from_here() {
    let place = Place::new();
    place.parted();
    // A rebase begun in a terminal - or by a pull - and stopped: furca reads
    // it, and goes on with it.
    place.git(&["switch", "--quiet", "feature"]);
    let stopped = run(&place.work(), &["rebase", "main"]);
    assert!(
        !stopped.status.success(),
        "the rebase stops at the conflict"
    );

    let repo = place.open();
    let halfway = repo.halfway().expect("reads").expect("halfway");
    assert_eq!(halfway.operation, Operation::Rebase);
    assert_eq!(
        halfway.branch.as_deref(),
        Some("feature"),
        "the branch being rewritten"
    );
    assert_eq!(
        halfway.commit.map(|c| c.subject),
        Some("feat: shout two".to_owned())
    );
    assert!(halfway.can_skip);

    let conflict = repo.conflict("f.txt").expect("reads");
    // Ours, in a rebase, is what the commits are replayed onto.
    assert_eq!(
        conflict.texts.as_ref().map(|t| t.ours.as_str()),
        Some("one\n2 from main\nthree\nfour\nFIVE\n")
    );
    let text = conflict.merged(Pick::Theirs).expect("in blocks");
    repo.git()
        .resolve("f.txt", &Resolution::Text { text })
        .expect("resolves");
    let stepped = repo.git().proceed(Proceed::Continue).expect("continues");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.branch.as_deref(), Some("feature"));
    assert_eq!(place.git(&["log", "-1", "--format=%s"]), "feat: shout two");
    assert_eq!(place.git(&["log", "-1", "--format=%P"]), place.rev("main"));
}
