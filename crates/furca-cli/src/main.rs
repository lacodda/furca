//! furca: the headless CLI door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.
//!
//! Every command prints for a person by default and for a program with
//! `--json`. The JSON is the core's own types serialized as they are, so the
//! CLI adds no second shape to keep in step.
//!
//! Exit status: 0 when the command did what was asked, 1 when it answered
//! "no" (a release plan that is not ready, a run that stopped, CI that is
//! not green), 2 when it could not answer at all - the same split as `grep`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use furca_core::{HeadSummary, Log, Refs, Repository, Tips};
use furca_release::hosting::{self, Forge, Place, Run, Waited};
use furca_release::{Config, Plan, Provider, Release, Report, Runner, Sources, Status};
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
    /// Push the current branch to the remote it follows.
    Push {
        #[command(flatten)]
        common: Common,
        /// Then wait for the CI runs the push starts, and say how they ended.
        #[arg(long)]
        wait_ci: bool,
    },
    /// Show the CI runs on a commit and how they ended.
    Checks {
        #[command(flatten)]
        common: Common,
        /// The commit: an id, a branch, a tag. Defaults to HEAD.
        #[arg(default_value = "HEAD")]
        revision: String,
    },
    /// Plan and run the next release from release.toml.
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
    /// Release: bump, changelog, gate, commit, push, wait for CI, tag, wait for the publish, record.
    Run {
        #[command(flatten)]
        common: Common,
    },
    /// Carry on a release that stopped after its commit, from the step it stopped at.
    Resume {
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
        Command::Push { common, wait_ci } => push(&common, wait_ci),
        Command::Checks { common, revision } => checks(&common, &revision),
        Command::Release {
            command: ReleaseCommand::Run { common },
        } => release_run(&common, false),
        Command::Release {
            command: ReleaseCommand::Resume { common },
        } => release_run(&common, true),
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

/// The forge behind a remote's address, asked over HTTPS.
fn forge(url: &str, provider: Option<Provider>) -> Option<Box<dyn Forge>> {
    let place = Place::from_url(url, provider)?;
    Some(Box::new(hosting::Http::new(place)))
}

/// A progress line on standard error: standard output carries the answer.
fn say(step: &str, line: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "  {step:<9}  {line}");
}

fn release_run(common: &Common, resume: bool) -> Result<Answer, String> {
    let release = Release::open(&common.repo).map_err(|e| e.to_string())?;
    let registries = furca_release::Http::default();
    let factory = |url: &str, provider: Provider| forge(url, Some(provider));
    let mut progress = |step: &str, line: &str| say(step, line);
    let mut runner = Runner {
        registries: &registries,
        record: Some(&furca_release::Rigger),
        forge: &factory,
        now: &std::time::SystemTime::now,
        sleep: &std::thread::sleep,
        gate_to_stderr: common.json,
        say: &mut progress,
    };
    let report = if resume {
        release.resume(&mut runner)
    } else {
        release.run(&mut runner)
    }
    .map_err(|e| e.to_string())?;
    emit(common.json, &report, report_text)?;
    Ok(if report.released {
        Answer::Yes
    } else {
        Answer::No
    })
}

/// What `push` did, for `--json`.
#[derive(Serialize)]
struct Pushed {
    commit: String,
    remote: String,
    branch: String,
    /// How the CI runs the push started ended; `null` without `--wait-ci`.
    ci: Option<Waited>,
}

fn push(common: &Common, wait_ci: bool) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let upstream = repo.upstream().map_err(|e| e.to_string())?.ok_or(
        "the current branch follows no remote: push it once with `git push --set-upstream`",
    )?;
    let commit = repo
        .head()
        .map_err(|e| e.to_string())?
        .commit
        .ok_or("there is nothing to push: no commits yet")?;
    let refspec = format!("HEAD:{}", upstream.merge);
    repo.git()
        .push(&upstream.remote, &[&refspec])
        .map_err(|e| e.to_string())?;
    say(
        "push",
        &format!(
            "{} to {} {}",
            short(&commit),
            upstream.remote,
            upstream.merge
        ),
    );

    let mut pushed = Pushed {
        commit: commit.clone(),
        remote: upstream.remote.clone(),
        branch: upstream.branch.clone(),
        ci: None,
    };
    if wait_ci {
        let forge = forge(&upstream.url, provider_of(&repo))
            .ok_or_else(|| not_a_forge(&upstream.remote, &upstream.url))?;
        let waited = hosting::wait(
            forge.as_ref(),
            &commit,
            &|run| run.reference.as_deref() == Some(upstream.branch.as_str()),
            &[],
            forge.pace(),
            &std::thread::sleep,
            &mut |runs| say("ci", &states(runs)),
        )?;
        pushed.ci = Some(waited);
    }
    emit(common.json, &pushed, pushed_text)?;
    Ok(match &pushed.ci {
        None | Some(Waited::Green { .. }) => Answer::Yes,
        Some(_) => Answer::No,
    })
}

/// What `checks` read, for `--json`.
#[derive(Serialize)]
struct Checks {
    commit: String,
    runs: Vec<Run>,
    /// Every run has ended and none of them failed.
    green: bool,
}

fn checks(common: &Common, revision: &str) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let commit = repo.commit(revision).map_err(|e| e.to_string())?.id;
    let upstream = repo.upstream().map_err(|e| e.to_string())?;
    let (remote, url) = match upstream {
        Some(upstream) => (upstream.remote, upstream.url),
        None => {
            let url = repo.remote_url("origin").map_err(|e| e.to_string())?.ok_or(
                "the current branch follows no remote and there is no `origin`: no forge to ask",
            )?;
            ("origin".to_owned(), url)
        }
    };
    let forge = forge(&url, provider_of(&repo)).ok_or_else(|| not_a_forge(&remote, &url))?;
    let runs = forge.runs(&commit)?;
    let green = !runs.is_empty() && runs.iter().all(|r| r.state.done() && !r.state.red());
    let checks = Checks {
        commit,
        runs,
        green,
    };
    emit(common.json, &checks, checks_text)?;
    Ok(if checks.green {
        Answer::Yes
    } else {
        Answer::No
    })
}

/// The provider `release.toml` names, when the repository has one.
fn provider_of(repo: &Repository) -> Option<Provider> {
    Config::load(repo.workdir()?).ok().map(|c| c.ci.provider)
}

fn not_a_forge(remote: &str, url: &str) -> String {
    format!("`{remote}` ({url}) is not a repository on GitHub or Forgejo")
}

fn states(runs: &[Run]) -> String {
    if runs.is_empty() {
        return "waiting for the runs to start".to_owned();
    }
    runs.iter()
        .map(|r| format!("{} {}", r.workflow, r.state.word()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn run_lines(runs: &[Run]) -> Vec<String> {
    let started = |r: &Run| format!("{} ({})", r.event, r.reference.as_deref().unwrap_or("-"));
    let workflow = runs.iter().map(|r| r.workflow.len()).max().unwrap_or(0);
    let by = runs.iter().map(|r| started(r).len()).max().unwrap_or(0);
    runs.iter()
        .map(|r| {
            format!(
                "  {:<9}  {:<workflow$}  {:<by$}  {}",
                r.state.word(),
                r.workflow,
                started(r),
                r.url
            )
        })
        .collect()
}

fn pushed_text(pushed: &Pushed) -> String {
    let mut out = vec![format!(
        "Pushed {} to {} {}",
        short(&pushed.commit),
        pushed.remote,
        pushed.branch
    )];
    if let Some(ci) = &pushed.ci {
        out.extend(run_lines(ci.runs()));
        out.push(match ci {
            Waited::Green { .. } => "CI is green.".to_owned(),
            Waited::Red { .. } => "CI failed.".to_owned(),
            Waited::Missing { .. } => "No CI run started for the push.".to_owned(),
            Waited::Timeout { .. } => "CI is still running; `furca checks` looks again.".to_owned(),
        });
    }
    out.join("\n")
}

fn checks_text(checks: &Checks) -> String {
    if checks.runs.is_empty() {
        return format!("No CI runs on {}", short(&checks.commit));
    }
    let mut out = vec![format!("CI on {}", short(&checks.commit))];
    out.extend(run_lines(&checks.runs));
    let running = checks.runs.iter().any(|r| !r.state.done());
    out.push(if checks.green {
        "Green.".to_owned()
    } else if checks.runs.iter().any(|r| r.state.red()) {
        "Red.".to_owned()
    } else if running {
        "Still running.".to_owned()
    } else {
        "Not green.".to_owned()
    });
    out.join("\n")
}

fn report_text(report: &Report) -> String {
    let mut out = Vec::new();
    let tag = report.tag.as_deref().unwrap_or("the next release");
    out.push(if report.released {
        format!("{} {tag} is out.", report.project)
    } else {
        format!("{} {tag} stopped.", report.project)
    });
    out.push(String::new());
    let width = report.steps.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for step in &report.steps {
        out.push(format!(
            "  {:<7}  {:<width$}  {}",
            serde_word(&step.status),
            step.name,
            step.summary
        ));
        for detail in &step.details {
            out.push(format!("  {:<7}  {:<width$}    {detail}", "", ""));
        }
    }
    if !report.after.is_empty() {
        out.push(String::new());
        out.push("Left for you:".to_owned());
        out.extend(report.after.iter().map(|a| format!("  {a}")));
    }
    if let Some(step) = report.stopped() {
        out.push(String::new());
        out.push(format!("Stopped at {}: {}", step.name, step.summary));
    }
    out.join("\n")
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
