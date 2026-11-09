//! Fetching, pulling, pushing and the remotes themselves, against throwaway
//! repositories: a working repository, the bare `origin` it follows, a bare
//! `mirror`, and another clone of `origin` that moves it the way a colleague
//! would - all in a temp directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use furca_core::{Error, Progress, Reconcile, Remotes, Repository};

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
        for bare in ["origin.git", "mirror.git"] {
            git(
                root,
                &["init", "--quiet", "--bare", "--initial-branch=main", bare],
            );
        }
        git(root, &["init", "--quiet", "--initial-branch=main", "work"]);
        configure(&place.work(), &root.join("no-hooks"));
        let work = place.work();
        git(&work, &["remote", "add", "origin", "../origin.git"]);
        place.write("README.md", "one\n");
        git(&work, &["add", "--all"]);
        git(&work, &["commit", "--quiet", "-m", "chore: begin"]);
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

    fn write(&self, path: &str, text: &str) {
        std::fs::write(self.work().join(path), text).expect("write");
    }

    fn open(&self) -> Repository {
        Repository::open(self.work()).expect("furca-core opens the repository")
    }

    /// A commit made in the other clone on what `origin`'s `main` holds now,
    /// and pushed there.
    fn colleague_pushes(&self, file: &str) -> String {
        let other = self.other();
        git(&other, &["fetch", "--quiet", "origin"]);
        git(&other, &["reset", "--quiet", "--hard", "origin/main"]);
        std::fs::write(other.join(file), "theirs\n").expect("write");
        git(&other, &["add", "--all"]);
        git(
            &other,
            &["commit", "--quiet", "-m", &format!("feat: add {file}")],
        );
        git(&other, &["push", "--quiet", "origin", "main"]);
        git(&other, &["rev-parse", "HEAD"])
    }

    fn commit(&self, file: &str) -> String {
        self.write(file, "ours\n");
        git(&self.work(), &["add", "--all"]);
        git(
            &self.work(),
            &["commit", "--quiet", "-m", &format!("feat: add {file}")],
        );
        git(&self.work(), &["rev-parse", "HEAD"])
    }

    fn bare(&self, name: &str, args: &[&str]) -> String {
        git(&self.dir.path().join(name), args)
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
        ("pull.rebase", "false"),
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
fn a_fetch_brings_what_a_colleague_pushed_and_says_what_moved() {
    let place = Place::new();
    let before = git(&place.work(), &["rev-parse", "origin/main"]);
    let theirs = place.colleague_pushes("theirs.txt");
    let repo = place.open();

    let mut heard = Vec::new();
    let fetched = repo
        .git()
        .fetch(&Remotes::Default, false, &mut |p: &Progress| {
            heard.push(p.clone())
        })
        .expect("the fetch goes through");

    assert_eq!(fetched.remotes, ["origin"]);
    let main = fetched
        .moved
        .iter()
        .find(|m| m.name == "refs/remotes/origin/main")
        .expect("origin/main moved");
    assert_eq!(main.from.as_deref(), Some(before.as_str()));
    assert_eq!(main.to.as_deref(), Some(theirs.as_str()));
    assert!(!heard.is_empty(), "git told how the fetch went");
    let run = repo.git_log().pop().expect("the fetch is in the log");
    assert_eq!(run.args[..2], ["fetch", "--progress"]);
    assert!(!run.said.contains('\r'), "{:?}", run.said);

    let again = repo
        .git()
        .fetch(&Remotes::Default, false, &mut deaf())
        .expect("fetches again");
    assert!(again.moved.is_empty(), "{:?}", again.moved);
}

#[test]
fn a_fetch_of_every_remote_asks_each_and_one_not_there_is_refused() {
    let place = Place::new();
    git(&place.work(), &["remote", "add", "mirror", "../mirror.git"]);
    git(
        &place.other(),
        &["push", "--quiet", "../mirror.git", "main"],
    );
    let repo = place.open();

    let fetched = repo
        .git()
        .fetch(&Remotes::All, false, &mut deaf())
        .expect("every remote is fetched");
    assert_eq!(fetched.remotes, ["mirror", "origin"]);
    assert!(
        fetched
            .moved
            .iter()
            .any(|m| m.name == "refs/remotes/mirror/main" && m.from.is_none()),
        "{:?}",
        fetched.moved
    );

    let refused = repo
        .git()
        .fetch(&Remotes::Named("upstream".to_owned()), false, &mut deaf());
    assert!(
        matches!(&refused, Err(Error::Remote(why)) if why.contains("upstream") && why.contains("mirror, origin")),
        "{refused:?}"
    );
}

#[test]
fn a_fetch_that_prunes_takes_away_a_branch_the_remote_no_longer_has() {
    let place = Place::new();
    git(
        &place.work(),
        &["push", "--quiet", "origin", "main:refs/heads/gone"],
    );
    git(&place.work(), &["fetch", "--quiet", "origin"]);
    place.bare("origin.git", &["branch", "--delete", "--force", "gone"]);
    let repo = place.open();

    let kept = repo
        .git()
        .fetch(&Remotes::Default, false, &mut deaf())
        .expect("fetches");
    assert!(
        kept.moved.is_empty(),
        "without prune it stays: {:?}",
        kept.moved
    );

    let pruned = repo
        .git()
        .fetch(&Remotes::Default, true, &mut deaf())
        .expect("fetches and prunes");
    assert_eq!(pruned.moved.len(), 1, "{:?}", pruned.moved);
    assert_eq!(pruned.moved[0].name, "refs/remotes/origin/gone");
    assert_eq!(pruned.moved[0].to, None);
}

#[test]
fn a_pull_brings_the_colleagues_commit_into_the_branch() {
    let place = Place::new();
    let theirs = place.colleague_pushes("theirs.txt");
    let repo = place.open();
    let before = repo.head().expect("reads").commit;

    let pulled = repo
        .git()
        .pull(Reconcile::AsConfigured, &mut deaf())
        .expect("the pull goes through");

    assert_eq!(pulled.branch, "main");
    assert_eq!(pulled.upstream, "origin/main");
    assert_eq!(pulled.from, before);
    assert_eq!(pulled.to.as_deref(), Some(theirs.as_str()));
    assert!(place.work().join("theirs.txt").exists());
}

#[test]
fn a_pull_puts_parted_branches_together_as_asked() {
    let place = Place::new();
    let theirs = place.colleague_pushes("theirs.txt");
    let ours = place.commit("ours.txt");
    let repo = place.open();

    let refused = repo.git().pull(Reconcile::FastForward, &mut deaf());
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(
        repo.head().expect("reads").commit.as_deref(),
        Some(ours.as_str())
    );

    let pulled = repo
        .git()
        .pull(Reconcile::Rebase, &mut deaf())
        .expect("rebases");
    let to = pulled.to.expect("a commit");
    assert_ne!(to, ours, "ours is replayed as a new commit");
    assert_eq!(git(&place.work(), &["rev-parse", "HEAD~1"]), theirs);
    assert!(place.work().join("ours.txt").exists());
    assert!(place.work().join("theirs.txt").exists());
}

#[test]
fn a_merging_pull_takes_gits_own_message_without_an_editor() {
    let place = Place::new();
    let theirs = place.colleague_pushes("theirs.txt");
    let ours = place.commit("ours.txt");
    let repo = place.open();

    let pulled = repo
        .git()
        .pull(Reconcile::Merge, &mut deaf())
        .expect("merges");
    let merge = pulled.to.expect("a merge commit");
    let parents = git(&place.work(), &["rev-list", "--parents", "-n", "1", &merge]);
    assert!(
        parents.contains(&ours) && parents.contains(&theirs),
        "{parents}"
    );
}

#[test]
fn a_pull_without_an_upstream_says_so() {
    let place = Place::new();
    git(&place.work(), &["checkout", "--quiet", "-b", "topic"]);
    let refused = place
        .open()
        .git()
        .pull(Reconcile::AsConfigured, &mut deaf());
    assert!(
        matches!(&refused, Err(Error::Remote(why)) if why.contains("topic follows no remote")),
        "{refused:?}"
    );
}

#[test]
fn a_new_branch_is_pushed_under_its_name_and_follows_it_from_then_on() {
    let place = Place::new();
    git(&place.work(), &["checkout", "--quiet", "-b", "topic"]);
    let commit = place.commit("topic.txt");
    let repo = place.open();

    let to = repo
        .destinations(&Remotes::Default)
        .expect("one destination");
    assert_eq!(to.len(), 1);
    assert_eq!(to[0].remote, "origin");
    assert_eq!(to[0].to, "refs/heads/topic");
    assert_eq!(to[0].tracking, "refs/remotes/origin/topic");
    assert!(to[0].set_upstream);

    let pushed = repo
        .git()
        .push_branch(&to[0], false, &mut deaf())
        .expect("pushes");
    assert_eq!(pushed.commit, commit);
    assert!(pushed.upstream_set);
    assert_eq!(place.bare("origin.git", &["rev-parse", "topic"]), commit);
    assert_eq!(
        git(&place.work(), &["config", "branch.topic.merge"]),
        "refs/heads/topic"
    );
    let now = place.open().destinations(&Remotes::Default).expect("reads");
    assert!(!now[0].set_upstream, "it follows origin/topic now");
}

#[test]
fn an_annotated_tag_on_the_history_goes_with_the_branch_and_a_plain_one_stays() {
    let place = Place::new();
    place.commit("release.txt");
    git(
        &place.work(),
        &["tag", "--annotate", "v1.0.0", "--message", "v1.0.0"],
    );
    git(&place.work(), &["tag", "bookmark"]);
    let repo = place.open();

    let to = repo.destinations(&Remotes::Default).expect("reads");
    repo.git()
        .push_branch(&to[0], false, &mut deaf())
        .expect("pushes");
    assert_eq!(place.bare("origin.git", &["tag", "--list"]), "v1.0.0");
}

#[test]
fn every_remote_gets_the_branch_and_only_the_default_one_is_followed() {
    let place = Place::new();
    git(&place.work(), &["remote", "add", "mirror", "../mirror.git"]);
    let commit = place.commit("both.txt");
    let repo = place.open();

    let to = repo.destinations(&Remotes::All).expect("reads");
    let remotes: Vec<&str> = to.iter().map(|d| d.remote.as_str()).collect();
    assert_eq!(remotes, ["mirror", "origin"]);
    assert!(
        to.iter().all(|d| !d.set_upstream),
        "main follows origin/main already"
    );
    for destination in &to {
        repo.git()
            .push_branch(destination, false, &mut deaf())
            .expect("pushes");
    }
    assert_eq!(place.bare("origin.git", &["rev-parse", "main"]), commit);
    assert_eq!(place.bare("mirror.git", &["rev-parse", "main"]), commit);

    git(&place.work(), &["checkout", "--quiet", "-b", "topic"]);
    let topic = place.open().destinations(&Remotes::All).expect("reads");
    let followed: Vec<&str> = topic
        .iter()
        .filter(|d| d.set_upstream)
        .map(|d| d.remote.as_str())
        .collect();
    assert_eq!(
        followed,
        ["origin"],
        "the default remote is the one followed"
    );
}

#[test]
fn a_push_goes_where_the_branch_is_pushed_and_a_choice_is_asked_for_when_there_is_none() {
    let place = Place::new();
    git(&place.work(), &["remote", "add", "mirror", "../mirror.git"]);
    git(
        &place.work(),
        &["config", "branch.main.pushRemote", "mirror"],
    );
    let to = place.open().destinations(&Remotes::Default).expect("reads");
    assert_eq!(to[0].remote, "mirror");
    assert_eq!(to[0].to, "refs/heads/main");

    git(&place.work(), &["remote", "rename", "origin", "upstream"]);
    git(
        &place.work(),
        &["config", "--unset", "branch.main.pushRemote"],
    );
    git(&place.work(), &["checkout", "--quiet", "-b", "topic"]);
    let refused = place.open().destinations(&Remotes::Default);
    assert!(
        matches!(&refused, Err(Error::Remote(why)) if why.contains("name the one")),
        "{refused:?}"
    );

    git(&place.work(), &["checkout", "--quiet", "--detach"]);
    let detached = place.open().destinations(&Remotes::Default);
    assert!(
        matches!(&detached, Err(Error::Remote(why)) if why.contains("no branch")),
        "{detached:?}"
    );
}

#[test]
fn a_push_with_a_lease_replaces_what_was_fetched_and_refuses_what_was_not() {
    let place = Place::new();
    place.colleague_pushes("theirs.txt");
    git(&place.work(), &["fetch", "--quiet", "origin"]);
    let ours = place.commit("ours.txt");
    let repo = place.open();
    let to = repo.destinations(&Remotes::Default).expect("reads");

    let plain = repo.git().push_branch(&to[0], false, &mut deaf());
    assert!(matches!(plain, Err(Error::Git { .. })), "{plain:?}");

    let forced = repo
        .git()
        .push_branch(&to[0], true, &mut deaf())
        .expect("the lease holds: origin has what was fetched");
    assert!(forced.forced);
    assert_eq!(place.bare("origin.git", &["rev-parse", "main"]), ours);

    // The colleague pushes again, and this clone has not fetched it.
    let theirs = place.colleague_pushes("again.txt");
    git(
        &place.work(),
        &[
            "commit",
            "--quiet",
            "--amend",
            "-m",
            "feat: add ours, again",
        ],
    );
    let refused = place.open().git().push_branch(&to[0], true, &mut deaf());
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");
    assert_eq!(
        place.bare("origin.git", &["rev-parse", "main"]),
        theirs,
        "what was not fetched is not lost"
    );
}

#[test]
fn a_remote_is_added_renamed_pointed_elsewhere_and_removed() {
    let place = Place::new();
    let repo = place.open();
    repo.git()
        .remote_add("mirror", "../mirror.git")
        .expect("adds");
    // A handle reads the configuration when it is opened.
    let repo = place.open();
    assert_eq!(repo.remote_names(), ["mirror", "origin"]);
    let refused = repo.git().remote_add("mirror", "../elsewhere.git");
    assert!(matches!(refused, Err(Error::Git { .. })), "{refused:?}");

    repo.git()
        .remote_rename("origin", "upstream")
        .expect("renames");
    let repo = place.open();
    assert_eq!(repo.remote_names(), ["mirror", "upstream"]);
    assert_eq!(
        git(&place.work(), &["config", "branch.main.remote"]),
        "upstream"
    );
    assert!(
        repo.refs()
            .expect("reads")
            .remote_branches
            .iter()
            .any(|b| b.name == "upstream/main"),
        "the remote-tracking branches move with it"
    );

    repo.git()
        .remote_set_url("mirror", "../origin.git")
        .expect("points elsewhere");
    let mirror = place
        .open()
        .remotes()
        .expect("reads")
        .into_iter()
        .find(|r| r.name == "mirror")
        .expect("mirror");
    assert!(mirror.url.is_some_and(|url| url.ends_with("origin.git")));

    place
        .open()
        .git()
        .remote_remove("upstream")
        .expect("removes");
    let repo = place.open();
    assert_eq!(repo.remote_names(), ["mirror"]);
    assert!(
        repo.upstream().expect("reads").is_none(),
        "main follows nothing now"
    );
    assert!(repo.refs().expect("reads").remote_branches.is_empty());
}

#[test]
fn a_prune_says_which_remote_tracking_branches_it_took_away() {
    let place = Place::new();
    git(
        &place.work(),
        &["push", "--quiet", "origin", "main:refs/heads/gone"],
    );
    git(&place.work(), &["fetch", "--quiet", "origin"]);
    place.bare("origin.git", &["branch", "--delete", "--force", "gone"]);

    let pruned = place.open().git().remote_prune("origin").expect("prunes");
    let names: Vec<&str> = pruned.moved.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["refs/remotes/origin/gone"]);
}
