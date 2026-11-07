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

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::Repository;
use crate::error::Error;

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
    /// What git printed on standard error, up to 16 KB of it.
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
        let heard = self.spawn(&args, None).map(|(run, _)| run);
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

    /// Pushes `refspecs` to `remote`.
    pub fn push(&self, remote: &str, refspecs: &[&str]) -> Result<(), Error> {
        let mut args = vec!["push", "--quiet", remote];
        args.extend_from_slice(refspecs);
        self.run(&args).map(drop)
    }

    /// Fetches `refspecs` from `remote`; all of its configured ones when
    /// `refspecs` is empty.
    pub fn fetch(&self, remote: &str, refspecs: &[&str]) -> Result<(), Error> {
        let mut args = vec!["fetch", "--quiet", remote];
        args.extend_from_slice(refspecs);
        self.run(&args).map(drop)
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
        let out = self.run(&["ls-remote", remote, name, &peeled])?;
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
        let (run, stdout) = self.spawn(args, input)?;
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
    fn spawn(&self, args: &[&str], input: Option<&[u8]>) -> Result<(GitRun, String), Error> {
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
        let output = child
            .wait_with_output()
            .map_err(|source| Error::GitMissing { source })?;
        if let Some(feeding) = feeding {
            let _ = feeding.join();
        }
        let run = GitRun {
            args: args.iter().map(|arg| without_passwords(arg)).collect(),
            code: output.status.code(),
            millis: started.elapsed().as_millis() as u64,
            at,
            input: said(input.unwrap_or_default()).replace('\0', "\n"),
            said: said(&output.stderr),
        };
        Ok((run, String::from_utf8_lossy(&output.stdout).into_owned()))
    }
}

/// Bytes as a run keeps them: trimmed, with no password in them, and cut at a
/// character.
fn said(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
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
/// once instead of waiting for an answer at a prompt nobody is watching.
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
    use super::{command_line, for_a_hook, said, without_passwords};

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
