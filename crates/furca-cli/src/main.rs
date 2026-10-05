//! furca: the headless CLI door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.
//!
//! Every command prints for a person by default and for a program with
//! `--json`. The JSON is the core's own types serialized as they are, so the
//! CLI adds no second shape to keep in step.
//!
//! Exit status: 0 when the command did what was asked, 1 when it answered
//! "no" (a release plan that is not ready), 2 when it could not answer at
//! all - the same split as `grep`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use furca_core::{HeadSummary, Log, Refs, Repository, Tips};
use furca_release::{Plan, Release, Sources, Status};
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
    /// Plan the next release from release.toml: the number and what stands in its way.
    Release {
        #[command(subcommand)]
        command: ReleaseCommand,
    },
}

#[derive(Subcommand)]
enum ReleaseCommand {
    /// Say what the next release is, what stands in its way and what a run would do. Changes nothing.
    Plan {
        #[command(flatten)]
        common: Common,
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

/// What a command answered, for the exit status.
enum Answer {
    Yes,
    No,
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(Answer::Yes) => ExitCode::SUCCESS,
        Ok(Answer::No) => ExitCode::from(1),
        Err(message) => {
            eprintln!("furca: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(command: Command) -> Result<Answer, String> {
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
        Command::Release {
            command: ReleaseCommand::Plan { common },
        } => {
            let release = Release::open(&common.repo).map_err(|e| e.to_string())?;
            let plan = release
                .plan(&Sources {
                    registries: Some(&furca_release::Http::default()),
                    record: Some(&furca_release::Rigger),
                    now: std::time::SystemTime::now(),
                })
                .map_err(|e| e.to_string())?;
            emit(common.json, &plan, plan_text)?;
            Ok(if plan.ready { Answer::Yes } else { Answer::No })
        }
    }
}

fn open(path: &Path) -> Result<Repository, String> {
    Repository::open(path).map_err(|e| e.to_string())
}

fn emit<T: Serialize>(json: bool, value: &T, text: fn(&T) -> String) -> Result<Answer, String> {
    let out = if json {
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    } else {
        text(value)
    };
    println!("{out}");
    Ok(Answer::Yes)
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

fn plan_text(plan: &Plan) -> String {
    let mut out = Vec::new();
    let released = plan
        .released
        .as_ref()
        .map_or("nothing released".to_owned(), ToString::to_string);
    match &plan.proposed {
        Some(proposed) => {
            let step = proposed
                .bump
                .map(|b| format!(", a {} step", serde_word(&b)))
                .unwrap_or_default();
            out.push(format!(
                "{} {released} -> {}{step}, from the {}",
                plan.project,
                proposed.version,
                serde_word(&proposed.source)
            ));
        }
        None => out.push(format!("{} {released} -> nothing to propose", plan.project)),
    }
    if let Some(stage) = &plan.stage {
        let due = stage
            .friday
            .as_deref()
            .map(|f| format!(", due {f}"))
            .unwrap_or_default();
        out.push(format!("stage v{} \"{}\"{due}", stage.version, stage.title));
    }
    out.push(String::new());

    let width = plan.checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for check in &plan.checks {
        out.push(format!(
            "  {:<4}  {:<width$}  {}",
            serde_word(&check.status),
            check.name,
            check.summary
        ));
        for detail in &check.details {
            out.push(format!("  {:<4}  {:<width$}    {detail}", "", ""));
        }
    }
    out.push(String::new());

    let steps = &plan.steps;
    let mut run = Vec::new();
    for edit in &steps.bump {
        run.push((
            "bump",
            format!("{} {}: {} -> {}", edit.path, edit.at, edit.from, edit.to),
        ));
    }
    if let Some(proposed) = &plan.proposed {
        run.push((
            "changelog",
            format!("a section for {} in {}", proposed.version, steps.changelog),
        ));
    }
    for command in &steps.gate {
        run.push(("gate", command.clone()));
    }
    if let Some(commit) = &steps.commit {
        run.push(("commit", commit.clone()));
    }
    run.push((
        "ci",
        format!(
            "wait for {} on {}",
            steps.ci.workflow,
            steps.ci.provider.label()
        ),
    ));
    if let Some(tag) = &steps.tag {
        let publish = steps
            .ci
            .publish
            .as_deref()
            .map(|w| format!(", which starts {w}"))
            .unwrap_or_default();
        run.push(("tag", format!("{tag}{publish}")));
    }
    for package in &steps.publish {
        let now = if package.first {
            " (not there yet: the first publish is by hand)".to_owned()
        } else {
            package
                .latest
                .as_ref()
                .map(|v| format!(" ({v} now)"))
                .unwrap_or_default()
        };
        run.push((
            "publish",
            format!("{} {}{now}", package.registry.label(), package.name),
        ));
    }
    for post in &steps.post {
        run.push(("after", serde_word(post)));
    }
    out.push("The run would".to_owned());
    let width = run.iter().map(|(step, _)| step.len()).max().unwrap_or(0);
    let mut last = "";
    for (step, what) in &run {
        let label = if *step == last { "" } else { step };
        out.push(format!("  {label:<width$}  {what}"));
        last = step;
    }
    out.push(String::new());

    let failing: Vec<&str> = plan
        .checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| c.name)
        .collect();
    if failing.is_empty() {
        out.push("Ready: nothing stands in the way.".to_owned());
    } else {
        out.push(format!("Not ready: {} fails.", failing.join(", ")));
    }
    out.join("\n")
}

/// The word a value serializes to, such as `minor` or `pass`: the text
/// output and the JSON name things the same.
fn serde_word<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(word)) => word,
        Ok(serde_json::Value::Object(map)) => map
            .get("kind")
            .and_then(|k| k.as_str())
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}
