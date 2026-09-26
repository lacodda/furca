//! A synthetic repository shaped like a long-lived project, built once and
//! reused: a mainline with feature branches merged back, branches still open,
//! remote-tracking copies, tags, and the odd commit from a slow clock.
//!
//! Synthetic rather than a clone of a real project: it needs no network, it is
//! the same on every machine and every run, and its size is a parameter.
//! Generation streams straight into `git fast-import`, which writes one pack.
//!
//! Shared by the budget benchmark and the ordering test; not every user
//! calls every function.

#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Bumped whenever the shape below changes, so a stale fixture on disk is
/// rebuilt instead of measured.
const SHAPE_VERSION: u32 = 4;

/// The branch every feature commit goes through while it is being written.
const SCRATCH: &str = "furca-fixture-scratch";

/// Files the history touches; each commit edits one of them.
const FILES: u32 = 200;

/// How the object database is laid out on disk. Reads cost very differently
/// across these, and a user's repository can be in any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One pack, packed refs and a commit-graph file: what `git gc` leaves
    /// behind on a current git.
    PackedWithGraph,
    /// The same without the commit-graph: an older git, or a repository
    /// with `core.commitGraph` off.
    Packed,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::PackedWithGraph => "packed+commit-graph",
            Layout::Packed => "packed",
        }
    }

    fn dir_name(self) -> &'static str {
        match self {
            Layout::PackedWithGraph => "graph",
            Layout::Packed => "pack",
        }
    }
}

/// Returns the fixture with `commits` commits in `layout`, building it under
/// `root` if it is not there yet.
pub fn ensure(root: &Path, commits: u32, layout: Layout) -> PathBuf {
    let dir = root.join(format!(
        "furca-fixture-v{SHAPE_VERSION}-{commits}-{}",
        layout.dir_name()
    ));
    // Inside `.git`, so the working tree stays clean: a stray file there
    // would be a change in every status the fixture is later measured on.
    let done = dir.join(".git").join("furca-fixture-complete");
    if done.exists() {
        return dir;
    }
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("remove a half-built fixture");
    }
    std::fs::create_dir_all(&dir).expect("fixture dir");

    git(&dir, &["init", "--quiet", "--initial-branch=main"]);
    import(&dir, commits);
    // What `git gc` leaves behind: one pack, no loose objects, the refs in
    // packed-refs. fast-import writes every ref as a file of its own, and on
    // Windows opening 500 files is a measurement of the file system.
    git(&dir, &["repack", "-a", "-d", "-q"]);
    git(&dir, &["pack-refs", "--all"]);
    match layout {
        Layout::PackedWithGraph => {
            git(&dir, &["commit-graph", "write", "--reachable"]);
        }
        Layout::Packed => {
            git(&dir, &["config", "core.commitGraph", "false"]);
        }
    }
    std::fs::write(&done, "").expect("mark the fixture complete");
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = git_command(dir).args(args).status().expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

/// `git` that ignores the machine's own configuration.
pub fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            dir.join(".git").join("no-global-config"),
        );
    command
}

/// A tiny deterministic generator: the fixture must be byte-identical on
/// every machine, so no randomness from the environment.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

struct Stream<W: Write> {
    out: W,
    next_mark: u32,
    written: u32,
    time: i64,
    rng: Rng,
}

impl<W: Write> Stream<W> {
    /// Writes one commit on `branch` with the given parents and returns its
    /// mark.
    fn commit(&mut self, branch: &str, parents: &[u32], subject: &str) -> u32 {
        let mark = self.next_mark;
        self.next_mark += 1;
        self.written += 1;
        // One commit in fifteen lands in the same second as the one before,
        // as a rebase or a scripted series does: ties in time are real, and
        // the order has to break them the way git does.
        if self.rng.below(15) != 0 {
            self.time += 60 + self.rng.below(3600) as i64;
        }
        // One commit in a hundred comes from a machine whose clock runs a
        // day behind - the case that breaks ordering by time alone.
        let time = if self.rng.below(100) == 0 {
            self.time - 86_400
        } else {
            self.time
        };
        let author = self.rng.below(12);
        let file = self.rng.below(u64::from(FILES));
        let content = format!("revision {mark}\n");

        let out = &mut self.out;
        writeln!(out, "commit refs/heads/{branch}").unwrap();
        writeln!(out, "mark :{mark}").unwrap();
        writeln!(
            out,
            "author Author {author} <author{author}@example.com> {time} +0000"
        )
        .unwrap();
        writeln!(
            out,
            "committer Author {author} <author{author}@example.com> {time} +0000"
        )
        .unwrap();
        writeln!(out, "data {}\n{subject}", subject.len()).unwrap();
        if let Some((first, rest)) = parents.split_first() {
            writeln!(out, "from :{first}").unwrap();
            for parent in rest {
                writeln!(out, "merge :{parent}").unwrap();
            }
        }
        writeln!(out, "M 100644 inline src/module_{file}.rs").unwrap();
        writeln!(out, "data {}\n{content}", content.len()).unwrap();
        mark
    }

    fn reset(&mut self, reference: &str, mark: u32) {
        writeln!(self.out, "reset {reference}\nfrom :{mark}\n").unwrap();
    }

    fn tag(&mut self, name: &str, mark: u32) {
        let message = format!("release {name}");
        let out = &mut self.out;
        writeln!(out, "tag {name}").unwrap();
        writeln!(out, "from :{mark}").unwrap();
        writeln!(
            out,
            "tagger Release <release@example.com> {} +0000",
            self.time
        )
        .unwrap();
        writeln!(out, "data {}\n{message}", message.len()).unwrap();
    }
}

fn import(dir: &Path, commits: u32) {
    let mut child = git_command(dir)
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("git fast-import starts");
    let stdin = std::io::BufWriter::new(child.stdin.take().expect("stdin"));
    let mut s = Stream {
        out: stdin,
        next_mark: 1,
        written: 0,
        time: 1_500_000_000,
        rng: Rng(0x9E37_79B9_7F4A_7C15),
    };

    let mut main = s.commit("main", &[], "initial commit");
    let mut feature = 0u32;
    let mut open = Vec::new();
    let mut released = 0u32;

    while s.written < commits {
        // Mainline work between features.
        for _ in 0..s.rng.below(4) + 1 {
            if s.written >= commits {
                break;
            }
            main = s.commit("main", &[main], "chore: keep the mainline moving");
        }

        // A feature branch from the current mainline, its commits
        // interleaved in time with further mainline work.
        feature += 1;
        // Every feature commits through one scratch ref; the branches that
        // stay open get their real names at the end. Merged ones need none,
        // as they are usually deleted after the merge.
        let branch = format!("feature/{feature}");
        let mut tip = main;
        for _ in 0..s.rng.below(12) + 1 {
            if s.written + 2 >= commits {
                break;
            }
            tip = s.commit(SCRATCH, &[tip], "feat: work on a feature");
            if s.rng.below(3) == 0 {
                main = s.commit("main", &[main], "fix: meanwhile on main");
            }
        }

        if tip != main && s.rng.below(20) == 0 {
            // Left open: still a ref, still a tip of the graph.
            open.push((branch, tip));
        } else if tip != main && s.written < commits {
            main = s.commit("main", &[main, tip], "merge a feature");
        }

        if s.written / 1000 > released {
            released = s.written / 1000;
            s.tag(&format!("v{}.0.0", released), main);
        }
    }

    // Only the last few open branches keep a local ref; all of them keep a
    // remote-tracking one, as after a fetch.
    let keep_local = open.len().saturating_sub(10);
    for (index, (branch, tip)) in open.iter().enumerate() {
        s.reset(&format!("refs/remotes/origin/{branch}"), *tip);
        if index >= keep_local {
            s.reset(&format!("refs/heads/{branch}"), *tip);
        }
    }
    s.reset("refs/remotes/origin/main", main);

    let mut out = s
        .out
        .into_inner()
        .map_err(|e| e.into_error())
        .expect("flush");
    out.flush().expect("flush");
    drop(out);
    let status = child.wait().expect("fast-import finishes");
    assert!(status.success(), "git fast-import failed");
    git(dir, &["update-ref", "-d", &format!("refs/heads/{SCRATCH}")]);
    git(dir, &["checkout", "--quiet", "--force", "main"]);
}
