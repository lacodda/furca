//! furca: the headless CLI door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.
//!
//! Every command prints for a person by default and for a program with
//! `--json`. The JSON is the core's own types serialized as they are, so the
//! CLI adds no second shape to keep in step.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use furca_core::{HeadSummary, Log, Refs, Repository, Tips};
use serde::Serialize;

#[derive(Parser)]
#[command(
    name = "furca",
    version,
    about = "A fast git client that stays out of your way."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show where HEAD is: branch, commit, upstream.
    Status {
        #[command(flatten)]
        common: Common,
    },
    /// List branches, remote-tracking branches and tags.
    Refs {
        #[command(flatten)]
        common: Common,
    },
    /// Show the history, newest first, parents after their children.
    Log {
        #[command(flatten)]
        common: Common,
        /// How many commits to show at most.
        #[arg(short = 'n', long, default_value_t = 100)]
        limit: usize,
        /// Start from every branch and tag, not only HEAD.
        #[arg(long)]
        all: bool,
    },
}

#[derive(Args)]
struct Common {
    /// Path inside the repository. Defaults to the current directory.
    #[arg(long, short = 'C', value_name = "PATH", default_value = ".")]
    repo: PathBuf,
    /// Print JSON instead of text.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("furca: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Command) -> Result<(), String> {
    match command {
        Command::Status { common } => {
            let head = open(&common.repo)?.head().map_err(|e| e.to_string())?;
            emit(common.json, &head, status_text)
        }
        Command::Refs { common } => {
            let refs = open(&common.repo)?.refs().map_err(|e| e.to_string())?;
            emit(common.json, &refs, refs_text)
        }
        Command::Log { common, limit, all } => {
            let tips = if all { Tips::All } else { Tips::Head };
            let log = open(&common.repo)?
                .log(tips, limit)
                .map_err(|e| e.to_string())?;
            emit(common.json, &log, log_text)
        }
    }
}

fn open(path: &Path) -> Result<Repository, String> {
    Repository::open(path).map_err(|e| e.to_string())
}

fn emit<T: Serialize>(json: bool, value: &T, text: fn(&T) -> String) -> Result<(), String> {
    let out = if json {
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    } else {
        text(value)
    };
    println!("{out}");
    Ok(())
}

fn short(id: &str) -> &str {
    &id[..id.len().min(7)]
}

fn status_text(head: &HeadSummary) -> String {
    let at = head.commit.as_deref().map(short);
    let mut out = match (&head.branch, at) {
        (Some(branch), Some(at)) => format!("On {branch} at {at}"),
        (Some(branch), None) => format!("On {branch}, no commits yet"),
        (None, Some(at)) => format!("HEAD detached at {at}"),
        (None, None) => "HEAD points nowhere".to_owned(),
    };
    if let Some(upstream) = &head.upstream {
        out.push_str(&format!(", tracking {upstream}"));
    }
    out
}

fn refs_text(refs: &Refs) -> String {
    let mut lines = Vec::new();
    for branch in &refs.branches {
        let mark = if branch.head { '*' } else { ' ' };
        let upstream = branch
            .upstream
            .as_deref()
            .map(|u| format!(" -> {u}"))
            .unwrap_or_default();
        lines.push(format!(
            "{mark} {} {}{upstream}",
            short(&branch.target),
            branch.name
        ));
    }
    for remote in &refs.remote_branches {
        lines.push(format!("  {} {}", short(&remote.target), remote.name));
    }
    for tag in &refs.tags {
        lines.push(format!("  {} tag: {}", short(&tag.target), tag.name));
    }
    if lines.is_empty() {
        return "No refs yet".to_owned();
    }
    lines.join("\n")
}

fn log_text(log: &Log) -> String {
    if log.commits.is_empty() {
        return "No commits yet".to_owned();
    }
    let mut lines: Vec<String> = log
        .commits
        .iter()
        .map(|commit| {
            // The date alone: the full time with its offset is in --json.
            let date = commit.author.time.get(..10).unwrap_or(&commit.author.time);
            format!(
                "{} {date} {}  {}",
                short(&commit.id),
                commit.author.name,
                commit.subject
            )
        })
        .collect();
    if log.truncated {
        lines.push(format!(
            "... more history; raise --limit (now {})",
            log.commits.len()
        ));
    }
    lines.join("\n")
}
