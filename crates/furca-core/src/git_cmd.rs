//! The one place furca runs the system `git` (ADR 0001).
//!
//! Reads go through gix in-process; everything that changes a repository or
//! talks to a remote runs `git` here, so hooks, credential helpers, signing
//! and the user's own configuration apply exactly as they do in a terminal.
//! Every run is written to the repository's command log, which is what a
//! console panel or a release report shows of what was done.
//!
//! Paths go to git on its standard input, taken literally: a file called
//! `*.txt` is that file and no other, and a list long enough to outgrow a
//! command line is still one run.
//!
//! What talks to a remote - fetch, pull, push, the remotes themselves - is in
//! `remotes`, and how far such a run has got is read as it runs (`progress`).

mod progress;
mod remotes;

pub use progress::Progress;

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::Repository;
use crate::error::Error;
use crate::stash::Stash;

/// Who is told how a run that talks to a remote is getting on.
pub type Hear<'a> = &'a mut dyn FnMut(&Progress);

/// One run of `git`, as the command log keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct GitRun {
    /// The arguments after `git`.
    pub args: Vec<String>,
    /// The exit code; `None` when git was stopped by a signal.
    pub code: Option<i32>,
    pub millis: u64,
    /// When it started, in milliseconds since the Unix epoch.
    pub at: u64,
    /// What it was given on standard input, as words, up to 16 KB of it: the
    /// paths of an `add`, a commit's message, a patch.
    pub input: String,
    /// What git printed on standard error, up to 16 KB of it, each line as a
    /// terminal would show it once git was done - a line of progress git
    /// rewrote a hundred times is its last rewrite; for a run that failed and
    /// printed nothing there, what it printed on standard output, where a
    /// stash that met a conflict names the files.
    pub said: String,
}

impl GitRun {
    /// The command as a person would type it: `git push origin HEAD:refs/heads/main`.
    pub fn command(&self) -> String {
        command_line(&self.args)
    }
}

/// The most of what went in and came out a run keeps: a console shows the
/// words, and a hook that prints a build's output is not worth holding on to.
const MOST_SAID: usize = 16 * 1024;

/// The most bytes of paths one run is given as arguments: Windows takes a
/// command line of 32 767 characters, and git's own words take some of it.
const MOST_ARGUMENTS: usize = 24 * 1024;

/// How many times a change to the index is tried when another program holds
/// its lock, and how long apart. An antivirus scanning `.git` on Windows and a
/// `git status` in another terminal both hold it for a moment.
const INDEX_TRIES: u32 = 8;
const INDEX_PAUSE: Duration = Duration::from_millis(150);

/// Who hears each run of `git` in this process, whichever handle made it.
type Hearing = Box<dyn Fn(&Path, &GitRun) + Send + Sync>;
static HEARING: OnceLock<Hearing> = OnceLock::new();

/// Has `hear` told of every run of `git` this process makes from now on, with
/// the repository's directory: a console panel, which sees the release engine's
/// pushes as well as its own commits. One per process; a second is ignored.
pub fn hear_git_runs(hear: impl Fn(&Path, &GitRun) + Send + Sync + 'static) {
    let _ = HEARING.set(Box::new(hear));
}

/// The runs of `git` made through one [`Repository`], oldest first.
#[derive(Debug, Default)]
pub(crate) struct CommandLog(Mutex<Vec<GitRun>>);

impl CommandLog {
    fn push(&self, run: GitRun) {
        if let Ok(mut runs) = self.0.lock() {
            runs.push(run);
        }
    }

    fn snapshot(&self) -> Vec<GitRun> {
        self.0.lock().map(|runs| runs.clone()).unwrap_or_default()
    }
}

/// The mutations of a repository, each one a run of the system `git`.
pub struct Git<'repo> {
    repo: &'repo Repository,
}

impl Repository {
    /// The door to the system `git` for this repository: commits, tags,
    /// pushes. Each call blocks until git exits.
    pub fn git(&self) -> Git<'_> {
        Git { repo: self }
    }

    /// Every run of `git` made through this handle so far, oldest first.
    pub fn git_log(&self) -> Vec<GitRun> {
        self.commands.snapshot()
    }
}

impl Git<'_> {
    /// Stages exactly `paths`, relative to the root of the working tree.
    pub fn add(&self, paths: &[&str]) -> Result<(), Error> {
        self.with_paths(&["add"], paths)
    }

    /// Stages every change the working tree holds, untracked files too.
    pub fn add_all(&self) -> Result<(), Error> {
        self.change_index(&["add", "--all"], None).map(drop)
    }

    /// Stages every change to a file git tracks, and no untracked file.
    pub fn add_tracked(&self) -> Result<(), Error> {
        self.change_index(&["add", "--update"], None).map(drop)
    }

    /// Takes `paths` out of the index again, leaving the files as they are.
    pub fn reset(&self, paths: &[&str]) -> Result<(), Error> {
        self.with_paths(&["reset", "--quiet"], paths)
    }

    /// Takes everything out of the index again, leaving the files as they are.
    pub fn reset_all(&self) -> Result<(), Error> {
        self.change_index(&["reset", "--quiet"], None).map(drop)
    }

    /// Applies `patch` to the index and to nothing else. It has to fit what the
    /// index holds: one written against anything else is refused, and the index
    /// stays as it was.
    pub fn apply_to_index(&self, patch: &[u8]) -> Result<(), Error> {
        self.change_index(
            &["apply", "--cached", "--whitespace=nowarn", "-"],
            Some(patch),
        )
        .map(drop)
    }

    /// Applies `patch` to the files of the working tree and to nothing else,
    /// through the same filters and line-ending conversion as a checkout. It
    /// has to fit what the files hold: one written against anything else is
    /// refused, and the files stay as they were.
    pub fn apply_to_working_tree(&self, patch: &[u8]) -> Result<(), Error> {
        self.run_with(&["apply", "--whitespace=nowarn", "-"], Some(patch))
            .map(drop)
    }

    /// Puts `paths` back in the working tree as the index has them: whatever
    /// was not staged of them is gone, and git keeps no copy of it.
    pub fn restore(&self, paths: &[&str]) -> Result<(), Error> {
        self.with_paths(&["restore", "--worktree"], paths)
    }

    /// Puts every file git tracks back in the working tree as the index has
    /// it. Untracked files stay.
    pub fn restore_all(&self) -> Result<(), Error> {
        self.change_index(&["restore", "--worktree", "--", "."], None)
            .map(drop)
    }

    /// Deletes the untracked files `paths`, taken as they are; a directory
    /// they leave empty stays. git has never stored them, so nothing keeps a
    /// copy. `git clean` reads no list from its input, so a long list is a few
    /// runs, each short enough for any system's command line.
    pub fn clean(&self, paths: &[&str]) -> Result<(), Error> {
        let mut rest = paths;
        while !rest.is_empty() {
            let mut length = 0;
            let take = rest
                .iter()
                .take_while(|path| {
                    length += path.len() + 3;
                    length <= MOST_ARGUMENTS
                })
                .count()
                .max(1);
            let (now, later) = rest.split_at(take);
            let mut args = vec!["--literal-pathspecs", "clean", "--force", "--quiet", "--"];
            args.extend_from_slice(now);
            self.run(&args)?;
            rest = later;
        }
        Ok(())
    }

    /// Deletes every untracked file that is not ignored, and the directories
    /// that held only those. A repository inside one stays.
    pub fn clean_all(&self) -> Result<(), Error> {
        self.run(&["clean", "--force", "--quiet", "-d"]).map(drop)
    }

    /// Puts every change away in a new stash entry - what is staged, what is
    /// not, and untracked files - with `message` when there is one, and leaves
    /// the working tree as `HEAD` has it. The entry made; `None` when there was
    /// nothing to put away.
    pub fn stash(&self, message: Option<&str>) -> Result<Option<Stash>, Error> {
        let before = self.repo.stashes()?.into_iter().next();
        let mut args = vec!["stash", "push", "--include-untracked"];
        if let Some(message) = message.map(str::trim).filter(|m| !m.is_empty()) {
            args.extend(["--message", message]);
        }
        self.change_index(&args, None)?;
        let after = self.repo.stashes()?.into_iter().next();
        Ok(after.filter(|made| before.as_ref().is_none_or(|was| was.commit != made.commit)))
    }

    /// Applies the stash entry `stash` to the working tree, and keeps it. A
    /// change that meets one the working tree has is a conflict, left in the
    /// files to be resolved, as git leaves it.
    pub fn stash_apply(&self, stash: &Stash) -> Result<(), Error> {
        let name = self.still(stash)?;
        self.change_index(&["stash", "apply", &name], None)
            .map(drop)
    }

    /// Applies the stash entry `stash` and drops it; one that met a conflict
    /// is kept, as git keeps it.
    pub fn stash_pop(&self, stash: &Stash) -> Result<(), Error> {
        let name = self.still(stash)?;
        self.change_index(&["stash", "pop", &name], None).map(drop)
    }

    /// Drops the stash entry `stash`. Its commit stays in the object store
    /// until git collects it, but nothing names it any more.
    pub fn stash_drop(&self, stash: &Stash) -> Result<(), Error> {
        let name = self.still(stash)?;
        self.run(&["stash", "drop", "--quiet", &name]).map(drop)
    }

    /// The name of `stash` - `stash@{2}` - once it is known to be the same
    /// entry still: one made or dropped since moves every entry after it.
    fn still(&self, stash: &Stash) -> Result<String, Error> {
        let name = format!("stash@{{{}}}", stash.index);
        let now = self.repo.stashes()?;
        match now.get(stash.index) {
            Some(entry) if entry.commit == stash.commit => Ok(name),
            _ => Err(Error::Moved { path: name }),
        }
    }

    /// Commits what is staged with `message` and returns the new commit's id.
    /// The author is git's own configuration, and the hooks run.
    pub fn commit(&self, message: &str) -> Result<String, Error> {
        self.commit_with(&["commit", "--quiet", "--file=-"], message)
    }

    /// Puts what is staged into the last commit, with `message` for it, and
    /// returns the id of the commit that takes its place. The hooks run.
    pub fn amend(&self, message: &str) -> Result<String, Error> {
        self.commit_with(&["commit", "--quiet", "--amend", "--file=-"], message)
    }

    fn commit_with(&self, args: &[&str], message: &str) -> Result<String, Error> {
        self.change_index(args, Some(message.as_bytes()))?;
        self.repo.head()?.commit.ok_or_else(|| Error::Git {
            command: command_line(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>()),
            code: None,
            stderr: "git reported a commit, and HEAD does not name one".to_owned(),
        })
    }

    /// What the repository's `commit-msg` hook says of `message`, without
    /// committing: `None` when it has nothing against it, or there is no hook,
    /// and otherwise its own words. This is the hook `git commit` will run -
    /// the same script, wherever `core.hooksPath` puts it - so a rule lives in
    /// one place and is asked here, not copied. It goes in no command log: it
    /// changes nothing, and it runs each time typing pauses.
    pub fn commit_msg_says(&self, message: &str) -> Result<Option<String>, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "FURCA_EDITMSG-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let file = self.repo.git_dir().join(name);
        std::fs::write(&file, message).map_err(|source| Error::Write {
            path: file.display().to_string(),
            source,
        })?;
        let shown = for_a_hook(&file);
        let args = [
            "hook",
            "run",
            "--ignore-missing",
            "commit-msg",
            "--",
            shown.as_str(),
        ];
        let heard = self.spawn(&args, None, None).map(|(run, _)| run);
        let _ = std::fs::remove_file(&file);
        let run = heard?;
        Ok(match run.code {
            Some(0) => None,
            // A git older than 2.36 has no `git hook`: nothing asked, nothing
            // said, and the commit will run the hook as it always has.
            _ if run.said.contains("is not a git command") => None,
            _ if run.said.is_empty() => Some(format!(
                "the commit-msg hook refused it{}",
                run.code.map(|c| format!(" (exit {c})")).unwrap_or_default()
            )),
            _ => Some(run.said),
        })
    }

    /// Makes the annotated tag `name` on `target` with `message`.
    pub fn tag(&self, name: &str, message: &str, target: &str) -> Result<(), Error> {
        self.run(&["tag", "--annotate", name, "--message", message, target])
            .map(drop)
    }

    /// Pushes exactly `refspecs` to `remote`, and nothing that follows them:
    /// what a release pushes, a commit and then its tag. A branch is pushed
    /// as a person pushes it with [`Git::push_branch`].
    pub fn push(&self, remote: &str, refspecs: &[&str]) -> Result<(), Error> {
        let mut args = vec!["push", "--quiet", remote];
        args.extend_from_slice(refspecs);
        self.reach(&args, None).map(drop)
    }

    /// Has git write the commit-graph file for every commit reachable from
    /// a ref, as a new layer over what is there (ADR 0010).
    pub fn write_commit_graph(&self) -> Result<(), Error> {
        self.run(&["commit-graph", "write", "--reachable", "--split"])
            .map(drop)
    }

    /// The object a reference names on `remote` right now, asked over the
    /// network; `None` when the remote has no such reference. An annotated
    /// tag is peeled to what it points at.
    pub fn remote_ref(&self, remote: &str, name: &str) -> Result<Option<String>, Error> {
        let peeled = format!("{name}^{{}}");
        let out = self.reach(&["ls-remote", remote, name, &peeled], None)?;
        let mut found = None;
        for line in out.lines() {
            let Some((id, reference)) = line.split_once('\t') else {
                continue;
            };
            if reference == peeled {
                return Ok(Some(id.to_owned()));
            }
            if reference == name {
                found = Some(id.to_owned());
            }
        }
        Ok(found)
    }

    /// Runs `git` on `paths`, listed on its standard input and taken as they
    /// are; none at all is nothing to do, where git would take it to mean every
    /// path there is.
    fn with_paths(&self, command: &[&str], paths: &[&str]) -> Result<(), Error> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["--literal-pathspecs"];
        args.extend_from_slice(command);
        args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        self.change_index(&args, Some(paths.join("\0").as_bytes()))
            .map(drop)
    }

    /// Runs what writes the index, trying again while something else holds its
    /// lock. Anything else that goes wrong is said at once.
    fn change_index(&self, args: &[&str], input: Option<&[u8]>) -> Result<String, Error> {
        let mut tries = 1;
        loop {
            match self.run_with(args, input) {
                Err(Error::Git { stderr, .. })
                    if tries < INDEX_TRIES && stderr.contains("index.lock") =>
                {
                    tries += 1;
                    std::thread::sleep(INDEX_PAUSE);
                }
                done => return done,
            }
        }
    }

    /// Runs `git args` in the working tree and returns what it printed on
    /// standard output. A failure carries git's own explanation.
    fn run(&self, args: &[&str]) -> Result<String, Error> {
        self.run_with(args, None)
    }

    fn run_with(&self, args: &[&str], input: Option<&[u8]>) -> Result<String, Error> {
        self.run_heard(args, input, None)
    }

    /// Runs `git args` that talks to a remote, telling `hear` how it is
    /// getting on when there is one to tell. A remote git could not get into
    /// fails with what furca knows of why: it never asks for a password, so
    /// the words say where git looks for one.
    fn reach(&self, args: &[&str], hear: Option<Hear<'_>>) -> Result<String, Error> {
        self.run_heard(args, None, hear)
            .map_err(|error| match error {
                Error::Git {
                    command, stderr, ..
                } if access_hint(&stderr).is_some() => Error::Access {
                    command,
                    hint: access_hint(&stderr).unwrap_or_default(),
                    stderr,
                },
                other => other,
            })
    }

    fn run_heard(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
        hear: Option<Hear<'_>>,
    ) -> Result<String, Error> {
        let (run, stdout) = self.spawn(args, input, hear)?;
        if let Some(hear) = HEARING.get() {
            hear(self.dir(), &run);
        }
        let failed = (run.code != Some(0)).then(|| Error::Git {
            command: run.command(),
            code: run.code,
            stderr: run.said.clone(),
        });
        self.repo.commands.push(run);
        match failed {
            Some(error) => Err(error),
            None => Ok(stdout),
        }
    }

    fn dir(&self) -> &Path {
        self.repo.workdir().unwrap_or_else(|| self.repo.git_dir())
    }

    /// Starts git, feeds it `input`, waits, and gives the run and what it
    /// printed. That git failed is in the run; `Err` is that it never started.
    ///
    /// Standard error is read as git writes it, so `hear` is told each line of
    /// progress as it comes; standard output is read beside it on a thread of
    /// its own, since git fills either pipe while the other is not read.
    fn spawn(
        &self,
        args: &[&str],
        input: Option<&[u8]>,
        mut hear: Option<Hear<'_>>,
    ) -> Result<(GitRun, String), Error> {
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64);
        let started = Instant::now();
        let mut command = git_command(self.dir());
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if input.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|source| Error::GitMissing { source })?;
        // Fed from a thread of its own: git may fill its pipes before it has
        // read all of this, and both would wait for ever.
        let feeding = input.zip(child.stdin.take()).map(|(input, mut stdin)| {
            let input = input.to_vec();
            std::thread::spawn(move || {
                // One that stops reading has its reason in its exit code.
                let _ = stdin.write_all(&input);
            })
        });
        let reading = child.stdout.take().map(|mut stdout| {
            std::thread::spawn(move || {
                let mut out = Vec::new();
                let _ = stdout.read_to_end(&mut out);
                out
            })
        });
        let mut stderr = Vec::new();
        if let Some(mut pipe) = child.stderr.take() {
            let mut buffer = [0u8; 4096];
            // Where the line being written began, which the next `\r` or
            // `\n` ends.
            let mut begun = 0;
            loop {
                let read = match pipe.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => read,
                };
                let scanned = stderr.len();
                stderr.extend_from_slice(&buffer[..read]);
                let Some(hear) = hear.as_mut() else {
                    continue;
                };
                for at in scanned..stderr.len() {
                    if matches!(stderr[at], b'\r' | b'\n') {
                        let line = String::from_utf8_lossy(&stderr[begun..at]);
                        if let Some(progress) = progress::progress(&line) {
                            hear(&progress);
                        }
                        begun = at + 1;
                    }
                }
            }
        }
        let status = child
            .wait()
            .map_err(|source| Error::GitMissing { source })?;
        let stdout = reading
            .and_then(|reading| reading.join().ok())
            .unwrap_or_default();
        if let Some(feeding) = feeding {
            let _ = feeding.join();
        }
        // A failure that says nothing on standard error says it on standard
        // output: a stash that met a conflict names the files there.
        let quiet = stderr.trim_ascii().is_empty();
        let words = if quiet && status.code() != Some(0) {
            &stdout
        } else {
            &stderr
        };
        let run = GitRun {
            args: args.iter().map(|arg| without_passwords(arg)).collect(),
            code: status.code(),
            millis: started.elapsed().as_millis() as u64,
            at,
            input: said(input.unwrap_or_default()).replace('\0', "\n"),
            said: said(words),
        };
        Ok((run, String::from_utf8_lossy(&stdout).into_owned()))
    }
}

/// What furca says of a remote git could not get into, by what git said of
/// it; `None` when git's own words are all there is to say. Read in git's
/// English and ssh's: a git that speaks another language gets its own words
/// alone.
fn access_hint(said: &str) -> Option<&'static str> {
    if said.contains("terminal prompts disabled") || said.contains("could not read Username") {
        Some(
            "git has no credential for this address, and furca never asks for one: \
             a credential helper keeps it (`git config --global credential.helper`), \
             or reach the remote over ssh with a key in ssh-agent",
        )
    } else if said.contains("Permission denied (publickey") {
        Some(
            "the server took none of the keys ssh offered: add yours to ssh-agent \
             (`ssh-add`), or name it for this host in ~/.ssh/config",
        )
    } else if said.contains("Host key verification failed") {
        Some(
            "ssh does not know this server's key yet, and furca does not ask: \
             connect once with `ssh` in a terminal and accept it there",
        )
    } else {
        None
    }
}

/// Bytes as a run keeps them: each line as a terminal would show it, trimmed,
/// with no password in them, and cut at a character.
fn said(bytes: &[u8]) -> String {
    let text = progress::as_shown(&String::from_utf8_lossy(bytes));
    let text = text.trim();
    let mut end = text.len().min(MOST_SAID);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    without_passwords(&text[..end])
}

/// `text` with whatever an address carries before its `@` taken out: a person
/// reads a console over a shoulder, and a token is as often the user as the
/// password.
pub(crate) fn without_passwords(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + 3);
        out.push_str(before);
        let end = after
            .find(|c: char| c == '/' || c == '\'' || c == '"' || c.is_whitespace())
            .unwrap_or(after.len());
        let authority = &after[..end];
        match authority.rfind('@') {
            Some(sign) => {
                out.push_str("***@");
                out.push_str(&authority[sign + 1..]);
            }
            None => out.push_str(authority),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// A path as a hook's shell can take it on any system: forward slashes, and on
/// Windows without the `\\?\` a resolved path may start with.
fn for_a_hook(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\")
        .unwrap_or(&text)
        .replace('\\', "/")
}

/// `git` set up to run unattended in `dir`: a missing credential fails at
/// once instead of waiting for an answer at a prompt nobody is watching, and
/// a merge a pull makes takes git's own message rather than opening an editor
/// nobody would close. Credentials are git's: its credential helper and
/// ssh-agent are asked as in a terminal, since the environment is passed on
/// as it is.
///
/// On Windows it also runs without a console window of its own. A program
/// built for the console, started from one that is not - the desktop app -
/// is given a fresh console by Windows, and every read the window had git
/// make flashed a black box over it. The output is captured either way.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`, from the Windows process creation flags.
        const NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(NO_WINDOW);
    }
    command
}

/// Arguments joined for reading, each one quoted when it would not survive
/// as a single word.
fn command_line(args: &[String]) -> String {
    let mut line = String::from("git");
    for arg in args {
        line.push(' ');
        if arg.is_empty() || arg.contains([' ', '"', '\'']) {
            line.push('"');
            line.push_str(&arg.replace('"', "\\\""));
            line.push('"');
        } else {
            line.push_str(arg);
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::{access_hint, command_line, for_a_hook, said, without_passwords};

    #[test]
    fn a_remote_git_could_not_get_into_says_where_a_credential_comes_from() {
        let https =
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled";
        assert!(access_hint(https).is_some_and(|hint| hint.contains("credential helper")));
        let key = "git@example.com: Permission denied (publickey).\r\nfatal: Could not read from remote repository.";
        assert!(access_hint(key).is_some_and(|hint| hint.contains("ssh-agent")));
        let host = "Host key verification failed.\nfatal: Could not read from remote repository.";
        assert!(access_hint(host).is_some_and(|hint| hint.contains("ssh")));
        let elsewhere = "fatal: unable to access 'https://example.com/x.git/': Could not resolve host: example.com";
        assert_eq!(access_hint(elsewhere), None);
    }

    #[test]
    fn a_line_git_rewrote_is_kept_as_it_was_left() {
        assert_eq!(
            said(b"Receiving objects:  50% (1/2)\rReceiving objects: 100% (2/2), done.\r\nTo origin\n"),
            "Receiving objects: 100% (2/2), done.\nTo origin"
        );
    }

    #[test]
    fn a_command_reads_as_it_would_be_typed() {
        let args = ["commit", "-m", "chore(release): v1.0.0", ""].map(String::from);
        assert_eq!(
            command_line(&args),
            r#"git commit -m "chore(release): v1.0.0" """#
        );
    }

    #[test]
    fn no_password_or_token_in_an_address_reaches_a_console() {
        assert_eq!(
            without_passwords("fatal: could not read from https://ada:s3cret@example.com/x.git"),
            "fatal: could not read from https://***@example.com/x.git"
        );
        assert_eq!(
            without_passwords("'https://ghp_token@github.com/a/b' and ssh://git@host:22/c"),
            "'https://***@github.com/a/b' and ssh://***@host:22/c"
        );
        assert_eq!(
            without_passwords("https://example.com/x and a@b: not an address"),
            "https://example.com/x and a@b: not an address"
        );
    }

    #[test]
    fn what_git_said_is_kept_within_reason_and_whole_characters() {
        let long = "é".repeat(super::MOST_SAID);
        let kept = said(long.as_bytes());
        assert!(kept.len() <= super::MOST_SAID, "{}", kept.len());
        assert!(kept.ends_with('é'));
        assert_eq!(said(b"  a hint\n"), "a hint");
    }

    #[test]
    fn a_hook_gets_a_path_its_shell_can_read() {
        assert_eq!(
            for_a_hook(std::path::Path::new(r"\\?\C:\work\.git\MSG")),
            "C:/work/.git/MSG"
        );
        assert_eq!(
            for_a_hook(std::path::Path::new("/work/.git/MSG")),
            "/work/.git/MSG"
        );
    }
}
