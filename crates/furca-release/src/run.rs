//! The run: the plan carried out step by step - bump, changelog, gate,
//! commit, push, CI, tag, publish, record - with a checkpoint after each.
//!
//! Before the release commit the steps are local and cheap, and a stop
//! there is undone rather than resumed: a red gate puts the files back as
//! they were, and the next run starts from a fresh plan. A fix made on top
//! of uncommitted release edits would ride into `chore(release)` and miss
//! the changelog written before it. After the commit the steps are remote
//! and slow - a push, a CI run, a tag, a publish - and `resume` carries a
//! stopped run on from the step it stopped at.
//!
//! The invariants hold whoever calls: a tag only on a commit whose CI is
//! green; nothing pushed is ever rewritten and a tag is never moved; the
//! changelog gets one new section and keeps the rest; the number is the
//! one the plan proved free.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use furca_core::GitRun;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::bump::{self, Touched};
use crate::changelog;
use crate::config::{Post, Provider, Registry};
use crate::error::Error;
use crate::hosting::{self, Forge, Page, Run, State, Waited};
use crate::manifest;
use crate::plan::{Release, Sources, Status};
use crate::record::{Record, Shipped};
use crate::registry::Registries;
use crate::version;

/// The checkpoint, inside the repository's `.git`: never committed, kept
/// between runs, one per repository.
pub const CHECKPOINT: &str = "furca-release.json";

/// The steps of a run, in order.
pub const STEPS: [&str; 10] = [
    "plan",
    "bump",
    "changelog",
    "gate",
    "commit",
    "push",
    "ci",
    "tag",
    "publish",
    "record",
];

/// How long the registries may take to show a version after its publish
/// workflow passed: crates.io's index lags its API by a minute or two.
const REGISTRY_LAG: Duration = Duration::from_secs(10 * 60);

/// The forge behind a remote's address, as `release.toml` names its
/// provider; `None` when the address is not one furca can read.
pub type ForgeFor<'a> = dyn Fn(&str, Provider) -> Option<Box<dyn Forge + 'a>> + 'a;

/// What a run asks of the world besides the repository.
pub struct Runner<'a> {
    pub registries: &'a dyn Registries,
    /// The owner's record; `None` to neither read nor write one.
    pub record: Option<&'a dyn Record>,
    pub forge: &'a ForgeFor<'a>,
    pub now: &'a dyn Fn() -> SystemTime,
    pub sleep: &'a dyn Fn(Duration),
    /// Sends what the gate's commands print on their standard output to
    /// standard error instead: for `--json`, whose standard output is one
    /// document.
    pub gate_to_stderr: bool,
    /// Told of each thing the run does as it does it: the step and a line.
    pub say: &'a mut dyn FnMut(&str, &str),
}

/// What a run did, step by step, and where it stopped if it did.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub project: String,
    pub version: Option<Version>,
    pub tag: Option<String>,
    /// The release is out: tagged, published, recorded.
    pub released: bool,
    /// Every step in order.
    pub steps: Vec<Step>,
    /// The release commit, once there is one.
    pub commit: Option<String>,
    /// The registries that hold the version.
    pub registries: Vec<String>,
    /// The release page the tag's workflows put out.
    pub page: Option<Page>,
    /// What is left to a person once the release is out.
    pub after: Vec<String>,
    /// Every git command the run made, in order.
    pub git: Vec<GitRun>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub name: &'static str,
    pub status: StepStatus,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StepStatus {
    Done,
    /// There was nothing to do: done before, or not asked for.
    Skipped,
    /// The run stopped here.
    Stopped,
    /// Not reached.
    Ahead,
}

impl Report {
    fn new(project: &str) -> Report {
        Report {
            project: project.to_owned(),
            version: None,
            tag: None,
            released: false,
            steps: STEPS
                .iter()
                .map(|name| Step {
                    name,
                    status: StepStatus::Ahead,
                    summary: String::new(),
                    details: Vec::new(),
                })
                .collect(),
            commit: None,
            registries: Vec::new(),
            page: None,
            after: Vec::new(),
            git: Vec::new(),
        }
    }

    fn mark(&mut self, name: &str, status: StepStatus, summary: String, details: Vec<String>) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.name == name) {
            step.status = status;
            step.summary = summary;
            step.details = details;
        }
    }

    /// Where the run stopped, if it did.
    pub fn stopped(&self) -> Option<&Step> {
        self.steps.iter().find(|s| s.status == StepStatus::Stopped)
    }
}

/// The checkpoint: what a run has done so far, written after every step.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Checkpoint {
    /// The shape of this file; another number is another furca's.
    format: u32,
    project: String,
    version: Version,
    tag: String,
    /// The last release's tag, if there is one.
    since: Option<String>,
    branch: String,
    remote: String,
    /// The branch on the remote, as a full name.
    merge: String,
    /// What the run has written and not yet committed.
    touched: Vec<Touched>,
    /// The release commit.
    commit: Option<String>,
    /// The commit last pushed.
    pushed: Option<String>,
    /// The commit CI passed on.
    green: Option<String>,
    /// The commit the tag names, once the tag is on the remote.
    tagged: Option<String>,
    /// The registries that hold the version, once they do.
    published: Option<Vec<String>>,
    page: Option<Page>,
}

const FORMAT: u32 = 1;

/// How a step ended: carry on, or stop - the report says where and why.
type Flow = Result<bool, Error>;

/// Run or resume.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Run,
    Resume,
}

impl Release {
    /// Plans the next release and carries it out: bump, changelog, gate,
    /// commit, push, CI, tag, publish, record. Stops at the first step that
    /// is not green; a stop before the commit takes its edits back, a stop
    /// after it is carried on by [`Release::resume`].
    pub fn run(&self, runner: &mut Runner) -> Result<Report, Error> {
        self.go(runner, Mode::Run)
    }

    /// Carries on a run that stopped after its commit, from the step it
    /// stopped at. A run that stopped before its commit has nothing to carry
    /// on: its edits are taken back and it starts over.
    pub fn resume(&self, runner: &mut Runner) -> Result<Report, Error> {
        self.go(runner, Mode::Resume)
    }

    fn checkpoint_path(&self) -> PathBuf {
        self.repo.git_dir().join(CHECKPOINT)
    }

    fn go(&self, runner: &mut Runner, mode: Mode) -> Result<Report, Error> {
        let mut report = Report::new(&self.config.project.name);
        let result = self.steps(runner, mode, &mut report);
        report.git = self.repo.git_log();
        result.map(|()| report)
    }

    fn steps(&self, runner: &mut Runner, mode: Mode, report: &mut Report) -> Result<(), Error> {
        let path = self.checkpoint_path();
        let mut state = load(&path)?;
        if let Some(saved) = &state {
            report.version = Some(saved.version.clone());
            report.tag = Some(saved.tag.clone());
            report.commit.clone_from(&saved.commit);
            if saved.commit.is_none() {
                let changed = bump::take_back(&self.root, &saved.touched)?;
                if !changed.is_empty() {
                    report.mark(
                        "plan",
                        StepStatus::Stopped,
                        format!(
                            "an interrupted run of {} left edits that were changed since",
                            saved.tag
                        ),
                        changed
                            .iter()
                            .map(|p| {
                                format!("{p}: put it back as it was, or commit it, then run again")
                            })
                            .chain([format!("then delete .git/{CHECKPOINT}")])
                            .collect(),
                    );
                    return Ok(());
                }
                (runner.say)(
                    "plan",
                    &format!("took back the edits of an interrupted run of {}", saved.tag),
                );
                remove(&path)?;
                state = None;
            } else if mode == Mode::Run {
                report.mark(
                    "plan",
                    StepStatus::Stopped,
                    format!(
                        "{} is under way past its commit: `furca release resume` carries it on",
                        saved.tag
                    ),
                    Vec::new(),
                );
                return Ok(());
            }
        } else if mode == Mode::Resume {
            report.mark(
                "plan",
                StepStatus::Stopped,
                "no release is under way here: `furca release run` starts one".to_owned(),
                Vec::new(),
            );
            return Ok(());
        }

        let mut state = match state {
            Some(state) => {
                for step in ["plan", "bump", "changelog", "gate", "commit"] {
                    report.mark(
                        step,
                        StepStatus::Done,
                        "before the resume".to_owned(),
                        Vec::new(),
                    );
                }
                state
            }
            None => match self.local(runner, report)? {
                Some(state) => state,
                None => return Ok(()),
            },
        };
        self.remote(runner, report, &mut state)
    }

    /// The steps up to the release commit. `None` when the run stopped.
    fn local(&self, runner: &mut Runner, report: &mut Report) -> Result<Option<Checkpoint>, Error> {
        let path = self.checkpoint_path();
        let Some(upstream) = self.repo.upstream()? else {
            report.mark(
                "plan",
                StepStatus::Stopped,
                "HEAD is not on a branch that follows a remote: the release has nowhere to go"
                    .to_owned(),
                Vec::new(),
            );
            return Ok(None);
        };
        let Some(head) = self.repo.head()?.commit else {
            report.mark(
                "plan",
                StepStatus::Stopped,
                "there are no commits".to_owned(),
                Vec::new(),
            );
            return Ok(None);
        };
        let changed = self.repo.changed_paths()?;
        if !changed.is_empty() {
            report.mark(
                "plan",
                StepStatus::Stopped,
                "the working tree has changes: commit them or put them aside first".to_owned(),
                changed,
            );
            return Ok(None);
        }
        if let Some(tip) = self
            .repo
            .git()
            .remote_ref(&upstream.remote, &upstream.merge)?
            && tip != head
            && !self.repo.is_ancestor(&tip, &head).unwrap_or(false)
        {
            report.mark(
                "plan",
                StepStatus::Stopped,
                format!(
                    "{} has commits this branch does not: pull them first",
                    upstream.tracking.as_deref().unwrap_or(&upstream.merge)
                ),
                Vec::new(),
            );
            return Ok(None);
        }

        let now = (runner.now)();
        let (plan, bumps) = self.survey(&Sources {
            registries: Some(runner.registries),
            record: runner.record,
            now,
        })?;
        let Some(proposed) = plan.proposed.clone() else {
            report.mark(
                "plan",
                StepStatus::Stopped,
                "nothing to release".to_owned(),
                plan.checks.iter().map(|c| c.summary.clone()).collect(),
            );
            return Ok(None);
        };
        report.version = Some(proposed.version.clone());
        report.tag = Some(proposed.tag.clone());
        let notes: Vec<String> = plan
            .checks
            .iter()
            .filter(|c| c.status == Status::Fail || c.status == Status::Warn)
            .flat_map(|c| {
                let word = if c.status == Status::Fail {
                    "fail"
                } else {
                    "warn"
                };
                std::iter::once(format!("{word} {}: {}", c.name, c.summary))
                    .chain(c.details.iter().map(|d| format!("  {d}")))
            })
            .collect();
        if !plan.ready {
            report.mark(
                "plan",
                StepStatus::Stopped,
                format!(
                    "{} is not ready: `furca release plan` says why",
                    proposed.tag
                ),
                notes,
            );
            return Ok(None);
        }
        let released = plan.released.as_ref().map(version::tag);
        let since = match released {
            Some(tag) if self.repo.commit(&tag).is_ok() => Some(tag),
            _ => None,
        };
        report.mark(
            "plan",
            StepStatus::Done,
            format!(
                "{} {} -> {}",
                plan.project,
                plan.released
                    .as_ref()
                    .map_or("nothing".to_owned(), ToString::to_string),
                proposed.version
            ),
            notes,
        );
        (runner.say)("plan", &format!("{} is ready", proposed.tag));

        let mut state = Checkpoint {
            format: FORMAT,
            project: plan.project.clone(),
            version: proposed.version.clone(),
            tag: proposed.tag.clone(),
            since,
            branch: upstream.branch.clone(),
            remote: upstream.remote.clone(),
            merge: upstream.merge.clone(),
            touched: Vec::new(),
            commit: None,
            pushed: None,
            green: None,
            tagged: None,
            published: None,
            page: None,
        };
        save(&path, &state)?;

        let to = proposed.version.to_string();
        if !self.bump(
            runner,
            report,
            &mut state,
            &bumps.fields,
            &bumps.mentions,
            &to,
        )? {
            return Ok(None);
        }
        if !self.changelog(runner, report, &mut state, now)? {
            return Ok(None);
        }
        if !self.gate(runner, report, &mut state)? {
            return Ok(None);
        }
        if !self.commit(runner, report, &mut state, &head)? {
            return Ok(None);
        }
        Ok(Some(state))
    }

    fn bump(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
        fields: &[manifest::Field],
        mentions: &[crate::texts::Mention],
        to: &str,
    ) -> Flow {
        if fields.is_empty() && mentions.is_empty() {
            report.mark(
                "bump",
                StepStatus::Skipped,
                format!("the files say {to} already"),
                Vec::new(),
            );
            return Ok(true);
        }
        let touched = bump::apply(&self.root, fields, mentions, to)?;
        state.touched = touched;
        save(&self.checkpoint_path(), state)?;
        let details: Vec<String> = fields
            .iter()
            .map(|f| format!("{} {}: {} -> {to}", f.path, f.key, f.version))
            .chain(
                mentions
                    .iter()
                    .map(|m| format!("{} line {}: {}", m.path, m.line, m.text)),
            )
            .collect();
        for line in &details {
            (runner.say)("bump", line);
        }

        // The manifests read back as the new number, every one of them.
        let tracked: BTreeSet<String> = self.repo.tracked_paths()?.into_iter().collect();
        let versions = manifest::versions(&self.root, &self.config.project.manifests, &|p| {
            tracked.contains(p)
        })?;
        let wrong: Vec<String> = versions
            .fields
            .iter()
            .filter(|f| f.version != to)
            .map(|f| format!("{} {} still says {}", f.path, f.key, f.version))
            .collect();
        if !wrong.is_empty() {
            self.stop_local(
                report,
                state,
                "bump",
                "the bump left versions behind".to_owned(),
                wrong,
            )?;
            return Ok(false);
        }
        report.mark(
            "bump",
            StepStatus::Done,
            format!("{} places say {to}", details.len()),
            details,
        );
        Ok(true)
    }

    fn changelog(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
        now: SystemTime,
    ) -> Flow {
        let relative = self.config.project.changelog.clone();
        let full = self.root.join(&relative);
        let before = std::fs::read_to_string(&full).map_err(|source| Error::Read {
            path: full.clone(),
            source,
        })?;
        let version = state.version.to_string();
        if changelog::has_section(&before, &version) {
            report.mark(
                "changelog",
                StepStatus::Skipped,
                format!("{relative} has its section for {version} already"),
                Vec::new(),
            );
            return Ok(true);
        }
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let section = changelog::section(
            &self.root,
            &self.repo,
            state.since.as_deref(),
            &state.tag,
            seconds,
        )?;
        if section.is_empty() {
            report.mark(
                "changelog",
                StepStatus::Skipped,
                format!(
                    "{} shows none of the commits, so {relative} gets no section",
                    changelog::CLIFF
                ),
                Vec::new(),
            );
            return Ok(true);
        }
        let after = changelog::insert(&before, &section);
        let file = Touched {
            path: relative.clone(),
            before,
            after,
        };
        bump::write(&self.root, std::slice::from_ref(&file))?;
        state.touched.push(file);
        save(&self.checkpoint_path(), state)?;
        let heading = section.lines().next().unwrap_or_default().to_owned();
        (runner.say)("changelog", &heading);
        report.mark(
            "changelog",
            StepStatus::Done,
            format!("{heading} in {relative}"),
            section
                .lines()
                .skip(1)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect(),
        );
        Ok(true)
    }

    fn gate(&self, runner: &mut Runner, report: &mut Report, state: &mut Checkpoint) -> Flow {
        let commands = self.config.gate.commands.clone();
        for command in commands.iter().filter(|c| !c.trim().is_empty()) {
            (runner.say)("gate", command);
            let code = shell(command, &self.root, runner.gate_to_stderr).map_err(|source| {
                Error::Gate {
                    command: command.clone(),
                    source,
                }
            })?;
            if code != Some(0) {
                let exit = code.map_or("was stopped".to_owned(), |c| format!("exited {c}"));
                self.stop_local(
                    report,
                    state,
                    "gate",
                    format!("`{command}` {exit}"),
                    vec![
                        "the release edits were taken back".to_owned(),
                        "fix it, commit the fix, then `furca release run` again".to_owned(),
                    ],
                )?;
                return Ok(false);
            }
        }

        // A gate checks; it does not edit. Anything it changed besides the
        // release's own edits would be left out of the release commit.
        let ours: BTreeSet<&str> = state.touched.iter().map(|t| t.path.as_str()).collect();
        let changed: BTreeSet<String> = self.repo.changed_paths()?.into_iter().collect();
        let mut odd: Vec<String> = changed
            .iter()
            .filter(|p| !ours.contains(p.as_str()))
            .map(|p| format!("{p}: changed by the gate"))
            .collect();
        for file in &state.touched {
            let now = std::fs::read_to_string(self.root.join(&file.path)).unwrap_or_default();
            if now != file.after {
                odd.push(format!(
                    "{}: the gate changed the release's edit",
                    file.path
                ));
            }
        }
        if !odd.is_empty() {
            self.stop_local(
                report,
                state,
                "gate",
                "the gate changed files: a gate checks, it does not edit".to_owned(),
                odd,
            )?;
            return Ok(false);
        }
        report.mark(
            "gate",
            StepStatus::Done,
            format!("{} commands passed", commands.len()),
            commands,
        );
        Ok(true)
    }

    fn commit(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
        head: &str,
    ) -> Flow {
        if state.touched.is_empty() {
            state.commit = Some(head.to_owned());
            save(&self.checkpoint_path(), state)?;
            report.commit = Some(head.to_owned());
            report.mark(
                "commit",
                StepStatus::Skipped,
                format!("nothing to commit: {} is the release commit", short(head)),
                Vec::new(),
            );
            return Ok(true);
        }
        let owned: Vec<String> = state.touched.iter().map(|t| t.path.clone()).collect();
        let paths: Vec<&str> = owned.iter().map(String::as_str).collect();
        let message = format!("chore(release): {}", state.tag);
        let made = self
            .repo
            .git()
            .add(&paths)
            .and_then(|()| self.repo.git().commit(&message));
        let id = match made {
            Ok(id) => id,
            Err(error) => {
                self.stop_local(
                    report,
                    state,
                    "commit",
                    "git did not make the release commit".to_owned(),
                    vec![
                        error.to_string(),
                        "the release edits were taken back".to_owned(),
                    ],
                )?;
                // Unstage what was added, now that the files are back.
                let _ = self.repo.git().reset(&paths);
                return Ok(false);
            }
        };
        state.commit = Some(id.clone());
        state.touched.clear();
        save(&self.checkpoint_path(), state)?;
        (runner.say)("commit", &format!("{} {message}", short(&id)));
        report.commit = Some(id.clone());
        report.mark(
            "commit",
            StepStatus::Done,
            format!("{} {message}", short(&id)),
            paths.iter().map(|p| (*p).to_owned()).collect(),
        );
        Ok(true)
    }

    /// Stops before the commit: the edits go back and the checkpoint goes.
    fn stop_local(
        &self,
        report: &mut Report,
        state: &Checkpoint,
        step: &str,
        summary: String,
        mut details: Vec<String>,
    ) -> Result<(), Error> {
        let changed = bump::take_back(&self.root, &state.touched)?;
        for path in changed {
            details.push(format!(
                "{path} was changed during the run and is left as it is"
            ));
        }
        remove(&self.checkpoint_path())?;
        report.mark(step, StepStatus::Stopped, summary, details);
        Ok(())
    }

    /// The steps after the release commit.
    fn remote(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
    ) -> Result<(), Error> {
        let url = self
            .repo
            .upstream()?
            .filter(|u| u.remote == state.remote)
            .map(|u| u.url)
            .unwrap_or_else(|| state.remote.clone());
        let Some(forge) = (runner.forge)(&url, self.config.ci.provider) else {
            report.mark(
                "push",
                StepStatus::Stopped,
                format!(
                    "`{}` ({url}) is not a repository on {} furca can read the CI of",
                    state.remote,
                    self.config.ci.provider.label()
                ),
                Vec::new(),
            );
            return Ok(());
        };
        let forge = forge.as_ref();

        if !self.push(runner, report, state)? {
            return Ok(());
        }
        if !self.ci(runner, report, state, forge)? {
            return Ok(());
        }
        if !self.tag(runner, report, state)? {
            return Ok(());
        }
        if !self.publish(runner, report, state, forge)? {
            return Ok(());
        }
        self.record(runner, report, state)?;
        remove(&self.checkpoint_path())?;
        report.released = true;
        report.after = self
            .config
            .post
            .iter()
            .map(|post| match post {
                Post::Install => {
                    "install the released build with the repository's installer".to_owned()
                }
                Post::Deploy => "deploy with turnout".to_owned(),
            })
            .collect();
        Ok(())
    }

    fn push(&self, runner: &mut Runner, report: &mut Report, state: &mut Checkpoint) -> Flow {
        let Some(commit) = state.commit.clone() else {
            return Ok(false);
        };
        let Some(head) = self.repo.head()?.commit else {
            return Ok(false);
        };
        if !self.repo.is_ancestor(&commit, &head)? {
            report.mark(
                "push",
                StepStatus::Stopped,
                format!("HEAD no longer holds the release commit {}", short(&commit)),
                vec![
                    "a release commit that was pushed stays: check out a branch that has it"
                        .to_owned(),
                ],
            );
            return Ok(false);
        }
        let tip = self.repo.git().remote_ref(&state.remote, &state.merge)?;
        if tip.as_deref() == Some(head.as_str()) {
            report.mark(
                "push",
                StepStatus::Skipped,
                format!("{} has {} already", state.remote, short(&head)),
                Vec::new(),
            );
        } else {
            if let Some(tip) = &tip
                && !self.repo.is_ancestor(tip, &head).unwrap_or(false)
            {
                report.mark(
                    "push",
                    StepStatus::Stopped,
                    format!(
                        "{} {} moved on: pull, then `furca release resume`",
                        state.remote, state.merge
                    ),
                    Vec::new(),
                );
                return Ok(false);
            }
            let refspec = format!("HEAD:{}", state.merge);
            self.repo.git().push(&state.remote, &[&refspec])?;
            (runner.say)(
                "push",
                &format!("{} to {} {}", short(&head), state.remote, state.merge),
            );
            report.mark(
                "push",
                StepStatus::Done,
                format!("{} to {} {}", short(&head), state.remote, state.merge),
                Vec::new(),
            );
        }
        state.pushed = Some(head);
        save(&self.checkpoint_path(), state)?;
        Ok(true)
    }

    fn ci(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
        forge: &dyn Forge,
    ) -> Flow {
        let Some(pushed) = state.pushed.clone() else {
            return Ok(false);
        };
        if state.green.as_deref() == Some(pushed.as_str()) {
            report.mark(
                "ci",
                StepStatus::Skipped,
                format!("green on {} already", short(&pushed)),
                Vec::new(),
            );
            return Ok(true);
        }
        let workflow = self.config.ci.workflow.clone();
        let branch = state.branch.clone();
        let say = &mut *runner.say;
        let waited = hosting::wait(
            forge,
            &pushed,
            &|run| run.reference.as_deref() == Some(branch.as_str()),
            &[workflow.as_str()],
            forge.pace(),
            runner.sleep,
            &mut |runs| say("ci", &states(runs)),
        )
        .map_err(Error::Forge)?;
        let lines = run_lines(waited.runs());
        match waited {
            Waited::Green { runs } => {
                let ours = runs.iter().find(|r| r.workflow == workflow);
                if ours.map(|r| r.state) != Some(State::Success) {
                    report.mark(
                        "ci",
                        StepStatus::Stopped,
                        format!(
                            "{workflow} did not pass on {}: a skipped run proves nothing",
                            short(&pushed)
                        ),
                        lines,
                    );
                    return Ok(false);
                }
                state.green = Some(pushed.clone());
                save(&self.checkpoint_path(), state)?;
                report.mark(
                    "ci",
                    StepStatus::Done,
                    format!("{workflow} passed on {}", short(&pushed)),
                    lines,
                );
                Ok(true)
            }
            Waited::Red { .. } => {
                let mut details = lines;
                details.push(
                    "the release commit is pushed and nothing is tagged: fix it with a new commit, then `furca release resume`".to_owned(),
                );
                report.mark(
                    "ci",
                    StepStatus::Stopped,
                    format!("CI failed on {}", short(&pushed)),
                    details,
                );
                Ok(false)
            }
            Waited::Missing { missing, .. } => {
                report.mark(
                    "ci",
                    StepStatus::Stopped,
                    format!(
                        "{} did not start on {}: does its trigger cover {branch}?",
                        missing.join(", "),
                        short(&pushed)
                    ),
                    vec![format!(
                        "start it by hand at {}, then `furca release resume`",
                        forge.actions_url()
                    )],
                );
                Ok(false)
            }
            Waited::Timeout { .. } => {
                let mut details = lines;
                details.push("`furca release resume` waits again".to_owned());
                report.mark(
                    "ci",
                    StepStatus::Stopped,
                    format!("CI is still running on {}", short(&pushed)),
                    details,
                );
                Ok(false)
            }
        }
    }

    fn tag(&self, runner: &mut Runner, report: &mut Report, state: &mut Checkpoint) -> Flow {
        let Some(green) = state.green.clone() else {
            return Ok(false);
        };
        let tag = state.tag.clone();
        if state.tagged.is_some() {
            report.mark(
                "tag",
                StepStatus::Skipped,
                format!("{tag} is out already"),
                Vec::new(),
            );
            return Ok(true);
        }

        // A tag cannot create a package its registry has never seen: the
        // first publish is by hand, from this very commit, before the tag.
        let mut first = Vec::new();
        for package in &self.config.package {
            match runner.registries.versions(package.registry, &package.name) {
                Ok(Some(_)) => {}
                Ok(None) => first.push(format!(
                    "{} `{}`: {}",
                    package.registry.label(),
                    package.name,
                    first_publish(package.registry, &package.name)
                )),
                Err(why) => {
                    report.mark(
                        "tag",
                        StepStatus::Stopped,
                        "could not ask the registries before the tag".to_owned(),
                        vec![why, "`furca release resume` asks again".to_owned()],
                    );
                    return Ok(false);
                }
            }
        }
        if !first.is_empty() {
            first.push(format!(
                "publish by hand from {} - the commit about to be tagged - then `furca release resume`",
                short(&green)
            ));
            report.mark(
                "tag",
                StepStatus::Stopped,
                "a package its registry has never seen cannot be published by a tag".to_owned(),
                first,
            );
            return Ok(false);
        }

        let name = format!("refs/tags/{tag}");
        let local = self
            .repo
            .refs()?
            .tags
            .into_iter()
            .find(|t| t.name == tag)
            .map(|t| t.target);
        let remote = self.repo.git().remote_ref(&state.remote, &name)?;
        for (place, target) in [("here", &local), (state.remote.as_str(), &remote)] {
            if let Some(target) = target
                && target != &green
            {
                report.mark(
                    "tag",
                    StepStatus::Stopped,
                    format!(
                        "{tag} {place} names {}, not {}: a tag is never moved",
                        short(target),
                        short(&green)
                    ),
                    Vec::new(),
                );
                return Ok(false);
            }
        }
        if local.is_none() {
            self.repo.git().tag(&tag, &tag, &green)?;
        }
        if remote.is_none() {
            self.repo.git().push(&state.remote, &[&name])?;
        }
        state.tagged = Some(green.clone());
        save(&self.checkpoint_path(), state)?;
        (runner.say)("tag", &format!("{tag} on {}", short(&green)));
        report.mark(
            "tag",
            StepStatus::Done,
            format!("{tag} on {}, pushed to {}", short(&green), state.remote),
            Vec::new(),
        );
        Ok(true)
    }

    fn publish(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &mut Checkpoint,
        forge: &dyn Forge,
    ) -> Flow {
        let Some(tagged) = state.tagged.clone() else {
            return Ok(false);
        };
        if let Some(published) = &state.published {
            report.registries.clone_from(published);
            report.page.clone_from(&state.page);
            report.mark(
                "publish",
                StepStatus::Skipped,
                "published already".to_owned(),
                Vec::new(),
            );
            return Ok(true);
        }
        let tag = state.tag.clone();
        let version = state.version.clone();

        // What the registries hold decides whether the packages are out: a
        // publish re-run by hand starts from another commit and the tag's own
        // runs never show it. Once they hold the version a red publish
        // workflow no longer matters - but the tag's other workflows, the
        // release page among them, still have to end green.
        let mut lines = Vec::new();
        let mut missing = self.missing_from_registries(runner.registries, &version)?;
        let publish = self.config.ci.publish.clone();
        let held = missing.is_empty();
        let keep = |run: &Run| {
            run.reference.as_deref() == Some(tag.as_str())
                && !(held && publish.as_deref() == Some(run.workflow.as_str()))
        };
        let mut expect: Vec<String> = match &publish {
            Some(workflow) if !held => vec![workflow.clone()],
            _ => Vec::new(),
        };
        if expect.is_empty() {
            // Nothing named to wait for: the runs the tag started, if any.
            for run in forge.runs(&tagged).map_err(Error::Forge)? {
                if keep(&run) && !expect.contains(&run.workflow) {
                    expect.push(run.workflow);
                }
            }
        }
        if !expect.is_empty() {
            let expected: Vec<&str> = expect.iter().map(String::as_str).collect();
            let say = &mut *runner.say;
            let waited = hosting::wait(
                forge,
                &tagged,
                &keep,
                &expected,
                forge.pace(),
                runner.sleep,
                &mut |runs| say("publish", &states(runs)),
            )
            .map_err(Error::Forge)?;
            lines = run_lines(waited.runs());
            let stop = match &waited {
                Waited::Green { .. } => None,
                Waited::Red { .. } => Some(format!("a workflow of {tag} failed")),
                Waited::Missing { missing, .. } => Some(format!(
                    "{} did not start for {tag}: is its tag trigger on?",
                    missing.join(", ")
                )),
                Waited::Timeout { .. } => Some(format!("the workflows of {tag} are still running")),
            };
            if let Some(summary) = stop {
                let mut details = lines;
                details.push(format!(
                    "{tag} is out and stays: fix the workflow and run it again for {tag}, then `furca release resume`"
                ));
                report.mark("publish", StepStatus::Stopped, summary, details);
                return Ok(false);
            }
        }

        // crates.io's index lags its API: ask until the version shows.
        let mut waited = Duration::ZERO;
        let pace = forge.pace();
        while !missing.is_empty() && waited < REGISTRY_LAG {
            (runner.sleep)(pace.every);
            waited += pace.every;
            missing = self.missing_from_registries(runner.registries, &version)?;
        }
        if !missing.is_empty() {
            let mut details = lines;
            details.extend(
                missing
                    .iter()
                    .map(|m| format!("{m} does not hold {version}")),
            );
            details.push("publish it, then `furca release resume`".to_owned());
            report.mark(
                "publish",
                StepStatus::Stopped,
                format!("{version} is not in every registry"),
                details,
            );
            return Ok(false);
        }

        let mut registries: Vec<String> = Vec::new();
        for package in &self.config.package {
            let label = package.registry.label().to_owned();
            if !registries.contains(&label) {
                registries.push(label);
            }
        }
        let page = forge.page(&tag).map_err(Error::Forge)?;
        state.published = Some(registries.clone());
        state.page.clone_from(&page);
        save(&self.checkpoint_path(), state)?;
        let summary = match (&page, registries.is_empty()) {
            (Some(p), _) if !p.draft => format!(
                "{version} is in {}; the release page has {} files",
                if registries.is_empty() {
                    "no registry".to_owned()
                } else {
                    registries.join(", ")
                },
                p.assets.len()
            ),
            (_, true) => "nothing is published to a registry".to_owned(),
            _ => format!("{version} is in {}", registries.join(", ")),
        };
        (runner.say)("publish", &summary);
        report.registries = registries;
        report.page = page;
        report.mark("publish", StepStatus::Done, summary, lines);
        Ok(true)
    }

    /// The packages whose registry does not hold `version` yet.
    fn missing_from_registries(
        &self,
        registries: &dyn Registries,
        version: &Version,
    ) -> Result<Vec<String>, Error> {
        let mut missing = Vec::new();
        for package in &self.config.package {
            let held = registries
                .versions(package.registry, &package.name)
                .map_err(Error::Forge)?
                .unwrap_or_default();
            if !held.contains(version) {
                missing.push(format!("{} `{}`", package.registry.label(), package.name));
            }
        }
        Ok(missing)
    }

    fn record(
        &self,
        runner: &mut Runner,
        report: &mut Report,
        state: &Checkpoint,
    ) -> Result<(), Error> {
        let Some(record) = runner.record else {
            report.mark(
                "record",
                StepStatus::Skipped,
                "no record was asked".to_owned(),
                Vec::new(),
            );
            return Ok(());
        };
        let shipped = Shipped {
            tag: state.tag.clone(),
            release: state
                .page
                .as_ref()
                .is_some_and(|p| !p.draft && !p.assets.is_empty()),
            registries: state.published.clone().unwrap_or_default(),
            text: format!(
                "released by `furca release run` from {}",
                state.commit.as_deref().map(short).unwrap_or_default()
            ),
        };
        match record.shipped(&self.config.project.name, &self.root, &shipped) {
            Ok(()) => {
                (runner.say)("record", &format!("{} shipped", state.tag));
                report.mark(
                    "record",
                    StepStatus::Done,
                    format!("the record has {} as shipped", state.tag),
                    Vec::new(),
                );
            }
            Err(why) => report.mark("record", StepStatus::Skipped, why, Vec::new()),
        }
        Ok(())
    }
}

/// How a package its registry has never seen is published the first time.
fn first_publish(registry: Registry, name: &str) -> String {
    match registry {
        Registry::Crates => format!("`cargo publish -p {name}`"),
        Registry::Npm => "`npm publish` in its directory".to_owned(),
    }
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
    runs.iter()
        .map(|r| format!("{} {}: {}", r.workflow, r.state.word(), r.url))
        .collect()
}

fn short(id: &str) -> &str {
    &id[..id.len().min(7)]
}

fn load(path: &Path) -> Result<Option<Checkpoint>, Error> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(Error::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let state: Checkpoint = serde_json::from_str(&text).map_err(|e| Error::Checkpoint {
        path: path.to_path_buf(),
        why: e.to_string(),
    })?;
    if state.format != FORMAT {
        return Err(Error::Checkpoint {
            path: path.to_path_buf(),
            why: format!("it is format {}, this furca reads {FORMAT}", state.format),
        });
    }
    Ok(Some(state))
}

fn save(path: &Path, state: &Checkpoint) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(state).map_err(|e| Error::Checkpoint {
        path: path.to_path_buf(),
        why: e.to_string(),
    })?;
    // Written aside and moved into place: a run killed mid-write leaves the
    // last checkpoint, never half of one.
    let aside = path.with_extension("json.new");
    std::fs::write(&aside, text).map_err(|source| Error::Write {
        path: aside.clone(),
        source,
    })?;
    std::fs::rename(&aside, path).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn remove(path: &Path) -> Result<(), Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::Write {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Runs one gate command in `dir` through the shell, its output on the
/// terminal: a gate is watched while it runs. Returns the exit code, `None`
/// when it was killed.
fn shell(command: &str, dir: &Path, to_stderr: bool) -> std::io::Result<Option<i32>> {
    #[cfg(windows)]
    let (program, flag) = (
        std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()),
        "/C",
    );
    #[cfg(not(windows))]
    let (program, flag) = (std::ffi::OsString::from("sh"), "-c");
    let mut cmd = std::process::Command::new(program);
    cmd.arg(flag).arg(command).current_dir(dir);
    if to_stderr {
        cmd.stdout(std::process::Stdio::from(std::io::stderr()));
    }
    Ok(cmd.status()?.code())
}
