//! The one place furca runs the system `git` (ADR 0001).
//!
//! Reads go through gix in-process; everything that changes a repository or
//! talks to a remote runs `git` here, so hooks, credential helpers, signing
//! and the user's own configuration apply exactly as they do in a terminal.
//! Every run is written to the repository's command log, which is what a
//! console panel or a release report shows of what was done.

use std::path::Path;
use std::process::Command;
use std::sync::Mutex;
use std::time::Instant;

use serde::Serialize;

use crate::Repository;
use crate::error::Error;

/// One run of `git`, as the command log keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GitRun {
    /// The arguments after `git`.
    pub args: Vec<String>,
    /// The exit code; `None` when git was stopped by a signal.
    pub code: Option<i32>,
    pub millis: u64,
}

impl GitRun {
    /// The command as a person would type it: `git push origin HEAD:refs/heads/main`.
    pub fn command(&self) -> String {
        command_line(&self.args)
    }
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
        let mut args = vec!["add", "--"];
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    /// Takes `paths` out of the index again, leaving the files as they are.
    pub fn reset(&self, paths: &[&str]) -> Result<(), Error> {
        let mut args = vec!["reset", "--quiet", "--"];
        args.extend_from_slice(paths);
        self.run(&args).map(drop)
    }

    /// Commits what is staged with `message` and returns the new commit's id.
    /// The author is git's own configuration, and the hooks run.
    pub fn commit(&self, message: &str) -> Result<String, Error> {
        self.run(&["commit", "--quiet", "-m", message])?;
        let head = self.run(&["rev-parse", "HEAD"])?;
        Ok(head.trim().to_owned())
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

    /// Runs `git args` in the working tree and returns what it printed on
    /// standard output. A failure carries git's own explanation.
    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let dir = self
            .repo
            .workdir()
            .unwrap_or_else(|| self.repo.git_dir())
            .to_path_buf();
        let started = Instant::now();
        let output = git_command(&dir)
            .args(args)
            .output()
            .map_err(|source| Error::GitMissing { source })?;
        let run = GitRun {
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            code: output.status.code(),
            millis: started.elapsed().as_millis() as u64,
        };
        let command = run.command();
        let code = run.code;
        self.repo.commands.push(run);
        if !output.status.success() {
            return Err(Error::Git {
                command,
                code,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// `git` set up to run unattended in `dir`: a missing credential fails at
/// once instead of waiting for an answer at a prompt nobody is watching.
fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null());
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
    use super::command_line;

    #[test]
    fn a_command_reads_as_it_would_be_typed() {
        let args = ["commit", "-m", "chore(release): v1.0.0", ""].map(String::from);
        assert_eq!(
            command_line(&args),
            r#"git commit -m "chore(release): v1.0.0" """#
        );
    }
}
