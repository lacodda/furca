//! Rebasing onto a branch and by a plan, squashing the last commits into one
//! and absorbing a change into a commit already made, against a throwaway
//! repository in a temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{
    Error, Operation, Picked, Proceed, RebaseAction, RebaseTodo, Repository, Resolution,
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

impl Place {
    /// Three more commits on `main`, each adding a file: `a.txt`, `b.txt`,
    /// `c.txt`. Their ids, oldest first.
    fn three(&self) -> [String; 3] {
        ["a", "b", "c"].map(|name| {
            self.write(&format!("{name}.txt"), &format!("{name}\n"));
            self.commit(&format!("feat: add {name}"))
        })
    }

    fn subjects(&self) -> Vec<String> {
        self.git(&["log", "--reverse", "--format=%s"])
            .lines()
            .map(ToOwned::to_owned)
            .collect()
    }
}

fn todo(commit: &str, action: RebaseAction) -> RebaseTodo {
    RebaseTodo {
        commit: commit.to_owned(),
        action,
    }
}

#[test]
fn a_branch_is_replayed_on_another_and_keeps_what_was_not_committed() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    place.write("h.txt", "h\n");
    let main = place.commit("feat: add h");
    place.git(&["switch", "--quiet", "feature"]);
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");

    let stepped = place.open().git().rebase("main").expect("rebases");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.stopped, None);
    assert_eq!(stepped.branch.as_deref(), Some("feature"));
    assert_eq!(place.rev("HEAD~1"), main, "replayed on main's tip");
    assert_eq!(
        place.subjects(),
        ["feat: start", "feat: add h", "feat: add g"]
    );
    assert_eq!(
        place.read("f.txt"),
        "one\ntwo\nthree\nfour\nfive\nsix\n",
        "the autostash brought the change back"
    );
}

#[test]
fn a_rebase_that_meets_a_conflict_stops_halfway_and_goes_on_once_resolved() {
    let place = Place::new();
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("f.txt", "one\nTWO from feature\nthree\nfour\nfive\n");
    let ours = place.commit("feat: shout two");
    place.write("g.txt", "g\n");
    place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    place.write("f.txt", "one\n2 from main\nthree\nfour\nfive\n");
    place.commit("feat: number two");
    place.git(&["switch", "--quiet", "feature"]);

    let repo = place.open();
    let stepped = repo.git().rebase("main").expect("a stop is an answer");
    assert_eq!(stepped.operation, Some(Operation::Rebase));
    assert_eq!(stepped.conflicts.len(), 1);

    let halfway = repo.halfway().expect("reads").expect("halfway");
    assert_eq!(halfway.operation, Operation::Rebase);
    assert_eq!(halfway.branch.as_deref(), Some("feature"));
    assert_eq!(
        halfway.commit.as_ref().map(|c| c.id.as_str()),
        Some(ours.as_str())
    );
    assert_eq!((halfway.step, halfway.steps), (Some(1), Some(2)));
    assert_eq!(halfway.text.as_deref(), Some("feat: shout two"));

    repo.git()
        .resolve("f.txt", &Resolution::Theirs)
        .expect("resolves");
    let stepped = repo.git().proceed(Proceed::Continue).expect("continues");
    assert_eq!(stepped.operation, None);
    assert_eq!(
        place.subjects(),
        [
            "feat: start",
            "feat: number two",
            "feat: shout two",
            "feat: add g"
        ]
    );
    assert_eq!(
        place.read("f.txt"),
        "one\nTWO from feature\nthree\nfour\nfive\n"
    );
}

#[test]
fn a_plan_lists_the_commits_oldest_first_with_their_messages() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, b, c] = place.three();
    place.git(&[
        "commit",
        "--quiet",
        "--amend",
        "-m",
        "feat: add c\n\nWith a body.",
    ]);
    let amended = place.rev("HEAD");
    assert_ne!(c, amended);

    let repo = place.open();
    let plan = repo.rebase_plan(Some(&start)).expect("plans");
    assert_eq!(plan.branch.as_deref(), Some("main"));
    assert_eq!(
        plan.onto.as_ref().map(|c| c.id.as_str()),
        Some(start.as_str())
    );
    let ids: Vec<&str> = plan.steps.iter().map(|s| s.commit.id.as_str()).collect();
    assert_eq!(ids, [a.as_str(), b.as_str(), amended.as_str()]);
    assert_eq!(plan.steps[2].message, "feat: add c\n\nWith a body.");
    assert!(plan.steps.iter().all(|step| !step.pushed));

    let whole = repo.rebase_plan(None).expect("plans from the root");
    assert_eq!(whole.onto, None);
    assert_eq!(whole.steps.len(), 4);
    assert_eq!(whole.steps[0].commit.id, start);
}

#[test]
fn a_plan_says_which_commits_the_upstream_has_already() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, _, _] = place.three();
    place.git(&["update-ref", "refs/remotes/origin/main", &a]);
    place.git(&["config", "remote.origin.url", "https://example.com/x.git"]);
    place.git(&[
        "config",
        "remote.origin.fetch",
        "+refs/heads/*:refs/remotes/origin/*",
    ]);
    place.git(&["config", "branch.main.remote", "origin"]);
    place.git(&["config", "branch.main.merge", "refs/heads/main"]);

    let plan = place.open().rebase_plan(Some(&start)).expect("plans");
    let pushed: Vec<bool> = plan.steps.iter().map(|step| step.pushed).collect();
    assert_eq!(pushed, [true, false, false]);
}

#[test]
fn a_merge_among_the_commits_is_refused() {
    let place = Place::new();
    let start = place.rev("HEAD");
    place.git(&["switch", "--quiet", "--create", "feature"]);
    place.write("g.txt", "g\n");
    place.commit("feat: add g");
    place.git(&["switch", "--quiet", "main"]);
    place.git(&["merge", "--quiet", "--no-ff", "--no-edit", "feature"]);

    let refused = place.open().rebase_plan(Some(&start));
    assert!(
        matches!(&refused, Err(Error::Operation(said)) if said.contains("is a merge")),
        "{refused:?}"
    );
}

#[test]
fn commits_are_reordered_reworded_folded_and_dropped_by_a_plan() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, b, c] = place.three();
    place.write("d.txt", "d\n");
    let d = place.commit("feat: add d");
    // Not committed, and not in the way: the autostash carries it over.
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");

    let repo = place.open();
    let stepped = repo
        .git()
        .rebase_interactive(
            Some(&start),
            &[
                todo(
                    &c,
                    RebaseAction::Reword {
                        message: "feat: add c first\n\nIt's \"quoted\", with a $dollar.".to_owned(),
                    },
                ),
                todo(&a, RebaseAction::Pick),
                todo(&d[..9], RebaseAction::Fixup),
                todo(&b, RebaseAction::Drop),
            ],
        )
        .expect("rebases");
    assert_eq!(stepped.operation, None, "{stepped:?}");
    assert_eq!(stepped.made, 2);
    assert_eq!(
        place.subjects(),
        ["feat: start", "feat: add c first", "feat: add a"]
    );
    assert_eq!(
        place.git(&["log", "-1", "--format=%B", "HEAD~1"]),
        "feat: add c first\n\nIt's \"quoted\", with a $dollar."
    );
    assert_eq!(
        place.git(&["show", "--format=", "--name-only", "HEAD"]),
        "a.txt\nd.txt",
        "d folded into a"
    );
    assert!(!place.work().join("b.txt").exists(), "b was dropped");
    assert_eq!(place.read("f.txt"), "one\ntwo\nthree\nfour\nfive\nsix\n");
    assert!(
        !place.work().join(".git/furca-rebase").exists(),
        "what the rebase read is gone with it"
    );
}

#[test]
fn a_squash_in_a_plan_keeps_both_messages() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, b, c] = place.three();

    place
        .open()
        .git()
        .rebase_interactive(
            Some(&start),
            &[
                todo(&a, RebaseAction::Pick),
                todo(&b, RebaseAction::Squash),
                todo(&c, RebaseAction::Pick),
            ],
        )
        .expect("rebases");
    assert_eq!(
        place.subjects(),
        ["feat: start", "feat: add a", "feat: add c"]
    );
    let message = place.git(&["log", "-1", "--format=%B", "HEAD~1"]);
    assert!(
        message.contains("feat: add a") && message.contains("feat: add b"),
        "{message}"
    );
}

#[test]
fn the_whole_history_is_replayed_from_its_first_commit() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, b, c] = place.three();

    place
        .open()
        .git()
        .rebase_interactive(
            None,
            &[
                todo(
                    &start,
                    RebaseAction::Reword {
                        message: "feat: begin".to_owned(),
                    },
                ),
                todo(&a, RebaseAction::Pick),
                todo(&b, RebaseAction::Pick),
                todo(&c, RebaseAction::Pick),
            ],
        )
        .expect("rebases");
    assert_eq!(
        place.subjects(),
        ["feat: begin", "feat: add a", "feat: add b", "feat: add c"]
    );
}

#[test]
fn a_plan_that_leaves_a_commit_out_or_names_a_stranger_is_refused() {
    let place = Place::new();
    let start = place.rev("HEAD");
    let [a, b, c] = place.three();
    let repo = place.open();
    let before = place.rev("HEAD");

    let left_out = repo.git().rebase_interactive(
        Some(&start),
        &[todo(&a, RebaseAction::Pick), todo(&c, RebaseAction::Pick)],
    );
    assert!(
        matches!(&left_out, Err(Error::Operation(said)) if said.contains("leaves out")),
        "{left_out:?}"
    );
    let stranger = repo.git().rebase_interactive(
        Some(&a),
        &[
            todo(&start, RebaseAction::Pick),
            todo(&b, RebaseAction::Pick),
            todo(&c, RebaseAction::Pick),
        ],
    );
    assert!(
        matches!(&stranger, Err(Error::Operation(said)) if said.contains("not one of")),
        "{stranger:?}"
    );
    let twice = repo.git().rebase_interactive(
        Some(&b),
        &[todo(&c, RebaseAction::Pick), todo(&c, RebaseAction::Pick)],
    );
    assert!(
        matches!(&twice, Err(Error::Operation(said)) if said.contains("twice")),
        "{twice:?}"
    );
    let nothing_before = repo.git().rebase_interactive(
        Some(&a),
        &[todo(&b, RebaseAction::Fixup), todo(&c, RebaseAction::Pick)],
    );
    assert!(
        matches!(&nothing_before, Err(Error::Operation(said)) if said.contains("folded")),
        "{nothing_before:?}"
    );
    assert_eq!(place.rev("HEAD"), before, "nothing moved");
    assert!(repo.halfway().expect("reads").is_none());
}

#[test]
fn a_plan_that_meets_a_conflict_stops_and_an_abort_puts_everything_back() {
    let place = Place::new();
    let start = place.rev("HEAD");
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
    let six = place.commit("feat: add six");
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\nseven\n");
    let seven = place.commit("feat: add seven");

    let repo = place.open();
    let stepped = repo
        .git()
        .rebase_interactive(
            Some(&start),
            &[
                todo(&seven, RebaseAction::Pick),
                todo(&six, RebaseAction::Pick),
            ],
        )
        .expect("a stop is an answer");
    assert_eq!(stepped.operation, Some(Operation::Rebase));
    assert_eq!(stepped.conflicts.len(), 1);
    assert!(place.work().join(".git/furca-rebase/todo").exists());

    let aborted = repo.git().proceed(Proceed::Abort).expect("aborts");
    assert_eq!(aborted.operation, None);
    assert_eq!(place.rev("HEAD"), seven);
    assert!(!place.work().join(".git/furca-rebase").exists());
}

#[test]
fn the_last_commits_become_one_with_their_messages_or_a_new_one() {
    let place = Place::new();
    let [a, _, _] = place.three();
    let author = place.git(&["log", "-1", "--format=%an %ad", &a]);
    // Not staged: it stays where it is.
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");

    let repo = place.open();
    let stepped = repo.git().squash(&a, None).expect("squashes");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.made, 1);
    assert_eq!(place.subjects(), ["feat: start", "feat: add a"]);
    assert_eq!(
        place.git(&["log", "-1", "--format=%B"]),
        "feat: add a\n\nfeat: add b\n\nfeat: add c"
    );
    assert_eq!(place.git(&["log", "-1", "--format=%an %ad"]), author);
    assert_eq!(
        place.git(&["show", "--format=", "--name-only", "HEAD"]),
        "a.txt\nb.txt\nc.txt"
    );
    assert_eq!(place.git(&["diff", "--name-only"]), "f.txt");

    place.git(&["checkout", "--quiet", "--", "f.txt"]);
    place.write("d.txt", "d\n");
    place.commit("feat: add d");
    let start = place.rev("HEAD~1");
    repo.git()
        .squash(&start, Some("feat: add four files"))
        .expect("squashes");
    assert_eq!(place.subjects(), ["feat: start", "feat: add four files"]);
}

#[test]
fn a_squash_is_refused_where_it_would_take_more_than_was_asked() {
    let place = Place::new();
    let [a, _, c] = place.three();
    let repo = place.open();

    let nothing_after = repo.git().squash(&c, None);
    assert!(
        matches!(&nothing_after, Err(Error::Operation(said)) if said.contains("nothing after")),
        "{nothing_after:?}"
    );
    place.write("d.txt", "d\n");
    place.git(&["add", "d.txt"]);
    let staged = repo.git().squash(&a, None);
    assert!(
        matches!(&staged, Err(Error::Operation(said)) if said.contains("staged")),
        "{staged:?}"
    );
    assert_eq!(place.rev("HEAD"), c);
}

#[test]
fn a_squash_a_hook_refuses_leaves_the_branch_where_it_was() {
    let place = Place::new();
    let [a, _, c] = place.three();
    let hooks = place.dir.path().join("hooks");
    std::fs::create_dir_all(&hooks).expect("hooks dir");
    std::fs::write(hooks.join("commit-msg"), "#!/bin/sh\necho no >&2\nexit 1\n").expect("hook");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            hooks.join("commit-msg"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("executable");
    }
    place.git(&["config", "core.hooksPath", hooks.to_str().expect("utf-8")]);

    let refused = place.open().git().squash(&a, None);
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(place.rev("HEAD"), c);
    assert_eq!(place.git(&["status", "--porcelain"]), "");
}

#[test]
fn what_is_staged_goes_into_a_commit_already_made() {
    let place = Place::new();
    let [a, b, c] = place.three();
    place.write("a.txt", "a\nand more\n");
    place.git(&["add", "a.txt"]);
    // Not staged: it stays out, and stays in the working tree.
    place.write("c.txt", "c\nnot yet\n");

    let stepped = place.open().git().absorb(&a).expect("absorbs");
    assert_eq!(stepped.operation, None);
    assert_eq!(stepped.from.as_deref(), Some(c.as_str()));
    assert_eq!(
        place.subjects(),
        ["feat: start", "feat: add a", "feat: add b", "feat: add c"]
    );
    assert_eq!(place.git(&["show", "HEAD~2:a.txt"]), "a\nand more");
    assert_ne!(place.rev("HEAD~1"), b, "the commits after it were replayed");
    assert_eq!(place.git(&["diff", "--name-only"]), "c.txt");
    assert_eq!(place.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(place.read("c.txt"), "c\nnot yet\n");
}

#[test]
fn lines_of_a_file_go_into_a_commit_and_what_is_staged_stays_out() {
    let place = Place::new();
    let [a, _, _] = place.three();
    place.write("b.txt", "b\nstaged\n");
    place.git(&["add", "b.txt"]);
    place.write("f.txt", "ONE\ntwo\nthree\nfour\nfive\n");
    place.write("a.txt", "a\nabsorbed\n");

    let repo = place.open();
    let status = repo.status().expect("status");
    let file = status
        .unstaged
        .iter()
        .find(|file| file.path == "a.txt")
        .expect("a.txt is changed");
    let diff = repo.diff(file).expect("diff");
    let picks: Vec<Picked> = diff.hunks.iter().map(|hunk| Picked::whole(*hunk)).collect();
    repo.git()
        .absorb_lines(file, &picks, &a)
        .expect("absorbs the lines");

    assert_eq!(place.git(&["show", "HEAD~2:a.txt"]), "a\nabsorbed");
    assert_eq!(
        place.git(&["show", "HEAD:b.txt"]),
        "b",
        "what was staged is in no commit"
    );
    assert_eq!(place.read("b.txt"), "b\nstaged\n");
    assert_eq!(place.read("f.txt"), "ONE\ntwo\nthree\nfour\nfive\n");
    assert_eq!(place.read("a.txt"), "a\nabsorbed\n");
    assert_eq!(place.git(&["diff", "HEAD", "--name-only"]), "b.txt\nf.txt");
    assert_eq!(
        place.git(&["diff", "--cached", "--name-only"]),
        "b.txt",
        "and is staged still"
    );
}

#[test]
fn a_change_the_later_commits_do_not_replay_over_is_refused_and_nothing_moves() {
    let place = Place::new();
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
    let six = place.commit("feat: add six");
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nSIX\n");
    let shout = place.commit("feat: shout six");
    // Rewrites the line the last commit rewrote: it cannot go under it.
    place.write("f.txt", "one\ntwo\nthree\nfour\nfive\nsix!\n");
    place.git(&["add", "f.txt"]);
    place.write("g.txt", "g\n");

    let repo = place.open();
    let refused = repo.git().absorb(&six);
    assert!(
        matches!(&refused, Err(Error::Operation(said)) if said.contains("Nothing was changed")),
        "{refused:?}"
    );
    assert_eq!(place.rev("HEAD"), shout);
    assert!(repo.halfway().expect("reads").is_none());
    assert_eq!(place.git(&["status", "--porcelain"]), "M  f.txt\n?? g.txt");
    assert_eq!(place.read("f.txt"), "one\ntwo\nthree\nfour\nfive\nsix!\n");

    place.git(&["reset", "--quiet", "--hard"]);
    let nothing = repo.git().absorb(&six);
    assert!(
        matches!(&nothing, Err(Error::Operation(said)) if said.contains("nothing to absorb")),
        "{nothing:?}"
    );
}
