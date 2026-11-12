//! Checking out, and making, renaming and deleting branches and tags, against
//! throwaway repositories: a working repository, the bare `origin` it follows,
//! and another clone of `origin` that pushes the way a colleague would - all
//! in a temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{Checkout, Error, Progress, Remotes, Repository};

struct Place {
    dir: tempfile::TempDir,
}

impl Place {
    fn new() -> Place {
        let place = Place {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        let root = place.dir.path();
        std::fs::create_dir_all(root.join("no-hooks")).expect("hooks dir");
        git(
            root,
            &[
                "init",
                "--quiet",
                "--bare",
                "--initial-branch=main",
                "origin.git",
            ],
        );
        git(root, &["init", "--quiet", "--initial-branch=main", "work"]);
        configure(&place.work(), &root.join("no-hooks"));
        let work = place.work();
        git(&work, &["remote", "add", "origin", "../origin.git"]);
        place.commit("README.md");
        git(
            &work,
            &["push", "--quiet", "--set-upstream", "origin", "main"],
        );
        git(root, &["clone", "--quiet", "origin.git", "other"]);
        configure(&place.other(), &root.join("no-hooks"));
        place
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    fn other(&self) -> PathBuf {
        self.dir.path().join("other")
    }

    fn open(&self) -> Repository {
        Repository::open(self.work()).expect("furca-core opens the repository")
    }

    /// A commit of `file` in the working repository, on whatever HEAD is on.
    fn commit(&self, file: &str) -> String {
        std::fs::write(self.work().join(file), format!("{file}\n")).expect("write");
        git(&self.work(), &["add", "--all"]);
        git(
            &self.work(),
            &["commit", "--quiet", "-m", &format!("feat: add {file}")],
        );
        self.rev("HEAD")
    }

    /// The branch `branch` pushed to `origin` from the other clone, with a
    /// commit of its own.
    fn colleague_pushes_branch(&self, branch: &str) -> String {
        let other = self.other();
        git(&other, &["checkout", "--quiet", "-b", branch]);
        let file = format!("{}.txt", branch.replace('/', "-"));
        std::fs::write(other.join(file), "theirs\n").expect("write");
        git(&other, &["add", "--all"]);
        git(&other, &["commit", "--quiet", "-m", "feat: theirs"]);
        git(&other, &["push", "--quiet", "origin", branch]);
        git(&other, &["rev-parse", "HEAD"])
    }

    fn rev(&self, revision: &str) -> String {
        git(&self.work(), &["rev-parse", revision])
    }

    fn config(&self, key: &str) -> Option<String> {
        let output = run(&self.work(), &["config", key]);
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn origin(&self, args: &[&str]) -> String {
        git(&self.dir.path().join("origin.git"), args)
    }
}

/// A repository's own configuration that shuts out the machine's.
fn configure(dir: &Path, hooks: &Path) {
    for (key, value) in [
        ("user.name", "Ada Author"),
        ("user.email", "ada@example.com"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.autocrlf", "false"),
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

/// Who hears nothing.
fn deaf() -> impl FnMut(&Progress) {
    |_: &Progress| {}
}

#[test]
fn a_branch_is_made_checked_out_and_takes_the_changes_that_do_not_meet_it() {
    let place = Place::new();
    let main = place.rev("HEAD");
    let repo = place.open();
    repo.git().branch_create("topic", "main").expect("makes it");
    assert_eq!(place.rev("topic"), main);
    assert_eq!(
        repo.head().expect("reads").branch.as_deref(),
        Some("main"),
        "making a branch leaves HEAD where it is"
    );

    std::fs::write(place.work().join("notes.txt"), "mine\n").expect("write");
    let to = repo.checkout_of("topic").expect("a branch");
    assert_eq!(
        to,
        Checkout::Branch {
            branch: "topic".to_owned()
        }
    );
    let switched = repo.git().checkout(&to).expect("checks it out");
    assert_eq!(switched.branch.as_deref(), Some("topic"));
    assert_eq!(switched.commit.as_deref(), Some(main.as_str()));
    assert!(!switched.created);
    assert!(
        place.work().join("notes.txt").exists(),
        "an untracked file goes along"
    );

    // A change to a file the other branch holds otherwise stops the checkout.
    let ahead = place.commit("topic.txt");
    git(&place.work(), &["checkout", "--quiet", "main"]);
    std::fs::write(place.work().join("topic.txt"), "in the way\n").expect("write");
    let repo = place.open();
    let refused = repo.git().checkout(&Checkout::Branch {
        branch: "topic".to_owned(),
    });
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(repo.head().expect("reads").branch.as_deref(), Some("main"));
    assert_ne!(place.rev("HEAD"), ahead);
}

#[test]
fn a_remote_branch_checked_out_is_a_local_branch_that_follows_it() {
    let place = Place::new();
    let theirs = place.colleague_pushes_branch("feature/login");
    git(&place.work(), &["fetch", "--quiet", "origin"]);
    let repo = place.open();

    let to = repo.checkout_of("origin/feature/login").expect("reads");
    assert_eq!(
        to,
        Checkout::New {
            branch: "feature/login".to_owned(),
            from: "origin/feature/login".to_owned(),
            follow: true,
        }
    );
    let switched = repo.git().checkout(&to).expect("checks it out");
    assert!(switched.created);
    assert_eq!(switched.branch.as_deref(), Some("feature/login"));
    assert_eq!(switched.commit.as_deref(), Some(theirs.as_str()));
    assert_eq!(switched.upstream.as_deref(), Some("origin/feature/login"));
    assert_eq!(
        place.config("branch.feature/login.merge").as_deref(),
        Some("refs/heads/feature/login")
    );

    // Once there is one that follows it, that one is checked out.
    git(&place.work(), &["checkout", "--quiet", "main"]);
    let again = place
        .open()
        .checkout_of("origin/feature/login")
        .expect("reads");
    assert_eq!(
        again,
        Checkout::Branch {
            branch: "feature/login".to_owned()
        }
    );

    // One of that name that follows nothing is not the remote's.
    git(
        &place.work(),
        &["branch", "--unset-upstream", "feature/login"],
    );
    let refused = place.open().checkout_of("origin/feature/login");
    assert!(
        matches!(&refused, Err(Error::Ref(why)) if why.contains("following nothing")),
        "{refused:?}"
    );
}

#[test]
fn a_new_branch_follows_only_the_remote_branch_whose_name_it_takes() {
    let place = Place::new();
    // git asked alone would have every new branch follow where it started.
    git(
        &place.work(),
        &["config", "branch.autoSetupMerge", "always"],
    );
    let repo = place.open();

    repo.git()
        .branch_create("fix", "origin/main")
        .expect("makes it");
    assert_eq!(
        place.config("branch.fix.merge"),
        None,
        "fix follows nothing"
    );
    repo.git().branch_create("local", "main").expect("makes it");
    assert_eq!(place.config("branch.local.merge"), None);

    let new = repo.checkout_new("main2", "origin/main").expect("reads");
    assert!(
        matches!(new, Checkout::New { follow: false, .. }),
        "{new:?}"
    );
    place.colleague_pushes_branch("next");
    git(&place.work(), &["fetch", "--quiet", "origin"]);
    let next = place
        .open()
        .checkout_new("next", "origin/next")
        .expect("reads");
    assert!(
        matches!(next, Checkout::New { follow: true, .. }),
        "{next:?}"
    );

    // A tag of the remote branch's short name does not stand in for it.
    git(&place.work(), &["tag", "origin/next", "main"]);
    let shadowed = place
        .open()
        .checkout_new("next", "origin/next")
        .expect("reads");
    assert!(
        matches!(shadowed, Checkout::New { follow: false, .. }),
        "{shadowed:?}"
    );
}

#[test]
fn a_name_that_looks_like_a_flag_is_a_name_and_git_refuses_it() {
    let place = Place::new();
    let repo = place.open();
    let refused = repo.git().branch_create("--force", "main");
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    let refused = repo.git().tag("-d", "main", None);
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(git(&place.work(), &["branch", "--list"]), "* main");
}

#[test]
fn a_commit_or_a_tag_is_checked_out_on_no_branch_and_only_when_asked() {
    let place = Place::new();
    let first = place.rev("HEAD");
    place.commit("two.txt");
    git(&place.work(), &["tag", "v1", &first]);
    let repo = place.open();

    let refused = repo.checkout_of("v1");
    assert!(
        matches!(&refused, Err(Error::Ref(why)) if why.contains("no branch")),
        "{refused:?}"
    );
    let typed = repo.checkout_of("HEAD~1");
    assert!(
        matches!(&typed, Err(Error::Ref(why)) if why.contains("no branch")),
        "a revision is no branch either: {typed:?}"
    );
    let to = repo.checkout_detached("v1").expect("reads");
    assert_eq!(
        to,
        Checkout::Detached {
            commit: first.clone()
        }
    );
    let switched = repo.git().checkout(&to).expect("checks it out");
    assert_eq!(switched.branch, None);
    assert_eq!(switched.commit.as_deref(), Some(first.as_str()));
    assert!(repo.head().expect("reads").detached);
}

#[test]
fn deleting_a_branch_says_what_only_it_holds_and_keeps_it_unless_forced() {
    let place = Place::new();
    git(&place.work(), &["checkout", "--quiet", "-b", "topic"]);
    place.commit("a.txt");
    let tip = place.commit("b.txt");
    git(&place.work(), &["checkout", "--quiet", "main"]);
    let repo = place.open();

    assert_eq!(repo.only_on("topic").expect("counts"), 2);
    assert_eq!(
        repo.only_on("main").expect("counts"),
        0,
        "origin/main has it"
    );
    let refused = repo.git().branch_delete("topic", false);
    assert!(
        matches!(&refused, Err(Error::Unmerged { branch, commits: 2 }) if branch == "topic"),
        "{refused:?}"
    );
    assert_eq!(place.rev("topic"), tip, "nothing went");

    let checked_out = repo.git().branch_delete("main", true);
    assert!(
        matches!(&checked_out, Err(Error::Ref(why)) if why.contains("checked out")),
        "{checked_out:?}"
    );

    // A tag keeps its commits: nothing is lost, and nothing has to be forced,
    // though git's own rule - merged into HEAD - would refuse it.
    git(&place.work(), &["tag", "kept", "topic"]);
    assert_eq!(place.open().only_on("topic").expect("counts"), 0);
    let deleted = place
        .open()
        .git()
        .branch_delete("topic", false)
        .expect("deletes");
    assert_eq!(deleted.target, tip);
    assert_eq!(deleted.lost, 0);

    git(&place.work(), &["checkout", "--quiet", "-b", "lone"]);
    let lone = place.commit("c.txt");
    git(&place.work(), &["checkout", "--quiet", "--detach", "main"]);
    assert_eq!(place.open().only_on("lone").expect("counts"), 1);
    let forced = place
        .open()
        .git()
        .branch_delete("lone", true)
        .expect("forced");
    assert_eq!((forced.target, forced.lost), (lone, 1));
    assert!(
        run(&place.work(), &["rev-parse", "--verify", "lone"])
            .status
            .code()
            != Some(0)
    );
}

#[test]
fn a_branch_is_renamed_with_what_it_follows_and_head_with_it() {
    let place = Place::new();
    place
        .open()
        .git()
        .branch_rename("main", "trunk")
        .expect("renames");
    let repo = place.open();
    assert_eq!(repo.head().expect("reads").branch.as_deref(), Some("trunk"));
    assert_eq!(
        repo.head().expect("reads").upstream.as_deref(),
        Some("origin/main")
    );
    git(&place.work(), &["branch", "taken"]);
    let refused = place.open().git().branch_rename("trunk", "taken");
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
}

#[test]
fn a_branch_is_set_to_follow_a_remote_branch_and_to_follow_nothing() {
    let place = Place::new();
    git(&place.work(), &["branch", "topic"]);
    place
        .open()
        .git()
        .branch_follow("topic", Some("origin/main"))
        .expect("follows");
    let topic = place
        .open()
        .refs()
        .expect("reads")
        .branches
        .into_iter()
        .find(|b| b.name == "topic")
        .expect("topic");
    assert_eq!(
        topic.upstream.map(|u| u.name).as_deref(),
        Some("origin/main")
    );

    place
        .open()
        .git()
        .branch_follow("topic", None)
        .expect("follows nothing");
    assert_eq!(place.config("branch.topic.merge"), None);
}

#[test]
fn tags_plain_and_annotated_are_made_pushed_and_taken_away_here_and_there() {
    let place = Place::new();
    let first = place.rev("HEAD");
    // A plain tag is never signed: asked to sign every tag, git would want a
    // message for this one and refuse.
    git(&place.work(), &["config", "tag.gpgSign", "true"]);
    let repo = place.open();
    repo.git().tag("bookmark", "HEAD", None).expect("tags");
    git(&place.work(), &["config", "tag.gpgSign", "false"]);
    repo.git()
        .tag("v1.0.0", "HEAD", Some("Release 1.0.0\n\nThe first.\n"))
        .expect("tags");
    let tags = place.open().refs().expect("reads").tags;
    let annotated: Vec<(&str, bool)> = tags
        .iter()
        .map(|t| (t.name.as_str(), t.annotated))
        .collect();
    assert_eq!(annotated, [("bookmark", false), ("v1.0.0", true)]);
    assert!(git(&place.work(), &["cat-file", "-p", "v1.0.0"]).contains("The first."));

    let repo = place.open();
    let remotes = repo.push_remotes(&Remotes::Default).expect("reads");
    assert_eq!(remotes, ["origin"]);
    let pushed = repo
        .git()
        .push_tag("origin", "bookmark", &mut deaf())
        .expect("pushes");
    assert!(!pushed.deleted);
    assert_eq!(place.origin(&["rev-parse", "bookmark"]), first);

    // The same name somewhere else is not put over what the remote has.
    git(&place.work(), &["tag", "--delete", "bookmark"]);
    let later = place.commit("later.txt");
    git(&place.work(), &["tag", "bookmark", &later]);
    let refused = place
        .open()
        .git()
        .push_tag("origin", "bookmark", &mut deaf());
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(place.origin(&["rev-parse", "bookmark"]), first);

    let repo = place.open();
    let gone = repo
        .git()
        .push_tag_delete("origin", "bookmark", &mut deaf())
        .expect("takes it away");
    assert!(gone.deleted);
    assert_eq!(place.origin(&["tag", "--list"]), "");
    let deleted = repo.git().tag_delete("bookmark").expect("deletes");
    assert_eq!(deleted.target, later);
    let missing = repo.git().push_tag("origin", "bookmark", &mut deaf());
    assert!(matches!(missing, Err(Error::Ref(_))), "{missing:?}");
}

#[test]
fn a_tag_goes_to_the_remote_the_branch_is_pushed_to_or_to_every_one() {
    let place = Place::new();
    git(
        place.dir.path(),
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            "mirror.git",
        ],
    );
    git(&place.work(), &["remote", "add", "mirror", "../mirror.git"]);
    git(
        &place.work(),
        &["config", "branch.main.pushRemote", "mirror"],
    );
    let repo = place.open();
    assert_eq!(
        repo.push_remotes(&Remotes::Default).expect("reads"),
        ["mirror"]
    );
    assert_eq!(
        repo.push_remotes(&Remotes::All).expect("reads"),
        ["mirror", "origin"]
    );

    // On no branch: `remote.pushDefault`, or else `origin`.
    git(&place.work(), &["checkout", "--quiet", "--detach"]);
    assert_eq!(
        place.open().push_remotes(&Remotes::Default).expect("reads"),
        ["origin"]
    );
    git(&place.work(), &["config", "remote.pushDefault", "mirror"]);
    assert_eq!(
        place.open().push_remotes(&Remotes::Default).expect("reads"),
        ["mirror"]
    );

    // Two remotes, neither of them `origin`, and nothing that says which.
    git(&place.work(), &["config", "--unset", "remote.pushDefault"]);
    git(&place.work(), &["remote", "rename", "origin", "upstream"]);
    let refused = place.open().push_remotes(&Remotes::Default);
    assert!(
        matches!(&refused, Err(Error::Remote(why)) if why.contains("name it")),
        "{refused:?}"
    );
}
