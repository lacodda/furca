//! A draft of a commit message from the Claude Code CLI on this machine.
//!
//! Opt-in twice over: the CLI is the person's own, with their session and their
//! plan, and nothing is sent until they press the button. Without the CLI there
//! is no button, and furca never asks for a key. The draft goes into the form to
//! be read and changed; it commits nothing.
//!
//! What it asks for is plain text on standard output, so no tool, no MCP server
//! and no session is needed, and none is left behind.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use furca_core::{Error, LineKind, Repository, Tips};

/// How much of the staged change goes into the prompt. A commit past this is
/// described by its list of files and the first part of its lines.
const MOST_LINES: usize = 60_000;
/// The subjects of recent commits, for the draft to write as this history does.
const RECENT: usize = 12;
/// A draft that takes longer than this is not going to come.
const PATIENCE: Duration = Duration::from_secs(120);

/// What the CLI is asked to do, apart from the change itself.
const BRIEF: &str = "Write a git commit message for the staged change below.

Follow Conventional Commits v1.0.0, in English:
- a subject of `type(scope): description`, 72 characters at most, with the \
description in the imperative mood, a lowercase first letter and no final full stop;
- the type is one of feat, fix, docs, style, refactor, perf, test, build, ci, \
chore, revert, picked by the effect of the change; leave the scope out when the \
change is cross-cutting;
- a `!` before the colon and a `BREAKING CHANGE:` footer only if it breaks something;
- a body only when the subject cannot carry why the change was made, after a \
blank line, wrapped at 72 characters. Say why, not what the diff already says.

No trailers, no attribution, no emoji. Reply with the message and nothing else: \
no code fence, no words around it.";

/// The name of the CLI's program here. An install through npm on Windows is a
/// `.cmd`, which `Command` does not look for by itself.
fn program() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

/// Whether the CLI is here.
pub fn available() -> bool {
    program().is_some()
}

/// Asks in a new process with no console of its own.
fn command(program: &Path) -> Command {
    // Only the Windows branch below changes it.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW`, from the Windows process creation flags.
        const NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(NO_WINDOW);
    }
    command
}

/// The prompt for what is staged in `repo`: the brief, recent subjects, the
/// files, and as many of the lines as fit.
pub fn prompt(repo: &Repository) -> Result<String, Error> {
    let staged = repo.status()?.staged;
    let mut out = String::from(BRIEF);
    if let Ok(log) = repo.log(Tips::Head, RECENT) {
        out.push_str("\n\nRecent subjects in this repository:\n");
        for commit in &log.commits {
            out.push_str(&format!("- {}\n", commit.subject));
        }
    }
    out.push_str("\nFiles staged:\n");
    for file in &staged {
        out.push_str(&format!("- {:?} {}\n", file.status, file.path));
    }
    out.push_str("\nThe change:\n");
    for file in &staged {
        if out.len() > MOST_LINES {
            out.push_str("\n(the rest of the lines are left out: the change is long)\n");
            break;
        }
        let patch = repo.diff(file)?.patch(3);
        out.push_str(&format!("\n--- {}\n", file.path));
        for hunk in &patch.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.before.start + 1,
                hunk.before.count,
                hunk.after.start + 1,
                hunk.after.count
            ));
            for line in &hunk.lines {
                let mark = match line.kind {
                    LineKind::Context => ' ',
                    LineKind::Removed => '-',
                    LineKind::Added => '+',
                };
                out.push(mark);
                out.push_str(line.text.trim_end_matches('\r'));
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// Runs the CLI on `prompt` from `root` and gives the message it wrote.
pub fn ask(root: &Path, prompt: &str) -> Result<String, String> {
    let program = program().ok_or("Claude Code is not on the PATH")?;
    let mut child = command(&program)
        // From standard input: Rust refuses a newline in an argument to a
        // `.cmd`, and a diff is mostly newlines.
        .args([
            "--print",
            "--output-format",
            "text",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--disable-slash-commands",
        ])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    let pid = child.id();
    if let Some(mut stdin) = child.stdin.take() {
        let prompt = prompt.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }
    // Waited for from a thread, so a CLI that hangs can be told apart from one
    // that is thinking.
    let (tell, told) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tell.send(child.wait_with_output());
    });
    let began = Instant::now();
    let output = loop {
        match told.recv_timeout(Duration::from_millis(250)) {
            Ok(output) => break output.map_err(|e| e.to_string())?,
            Err(_) if began.elapsed() < PATIENCE => {}
            Err(_) => {
                stop(pid);
                return Err(format!(
                    "Claude Code had not answered after {} s",
                    PATIENCE.as_secs()
                ));
            }
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let why = [stderr.trim(), stdout.trim()]
            .into_iter()
            .find(|text| !text.is_empty())
            .unwrap_or("no reason given");
        return Err(format!("Claude Code did not write a draft: {why}"));
    }
    let reply = unfenced(&String::from_utf8_lossy(&output.stdout));
    if reply.is_empty() {
        return Err("Claude Code wrote nothing".to_owned());
    }
    Ok(reply)
}

/// Stops the CLI and what it started: on Windows `claude.cmd` is a shell in front
/// of the program.
fn stop(pid: u32) {
    let mut sweep = if cfg!(windows) {
        let mut sweep = Command::new("taskkill");
        sweep.args(["/T", "/F", "/PID", &pid.to_string()]);
        sweep
    } else {
        let mut sweep = Command::new("kill");
        sweep.args(["-9", &pid.to_string()]);
        sweep
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        sweep.creation_flags(NO_WINDOW);
    }
    let _ = sweep.stdin(Stdio::null()).output();
}

/// The reply without a fence around it, which a model adds however it is asked.
fn unfenced(reply: &str) -> String {
    let reply = reply.trim();
    let Some(inside) = reply.strip_prefix("```") else {
        return reply.to_owned();
    };
    let inside = inside.split_once('\n').map_or("", |(_, rest)| rest);
    inside
        .strip_suffix("```")
        .unwrap_or(inside)
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::unfenced;

    #[test]
    fn a_fence_around_a_reply_is_taken_off() {
        assert_eq!(unfenced("feat: a thing\n"), "feat: a thing");
        assert_eq!(unfenced("```\nfeat: a thing\n```"), "feat: a thing");
        assert_eq!(
            unfenced("```text\nfeat: a thing\n\nA body.\n```\n"),
            "feat: a thing\n\nA body."
        );
        assert_eq!(unfenced("``` "), "");
    }
}
