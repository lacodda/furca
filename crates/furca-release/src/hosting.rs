//! The forge a repository is pushed to, as far as a release needs it: which
//! workflow runs a commit has and how they ended, and whether a release page
//! went out for a tag.
//!
//! Two forges are read: GitHub (Actions) and Forgejo (Forgejo Actions, the
//! `actions/runs` API of Forgejo 11 and later). Both are asked over HTTPS
//! with a token when there is one - `GH_TOKEN`, `GITHUB_TOKEN` or the `gh`
//! CLI's own for GitHub, `FORGEJO_TOKEN` for Forgejo - and anonymously
//! otherwise, which a public repository allows at a lower rate. A token is
//! sent and never printed.

use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::config::Provider;

/// One workflow run on a commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run {
    pub id: u64,
    /// The workflow's file name, such as `ci.yml`.
    pub workflow: String,
    /// The name the workflow gives itself.
    pub name: String,
    /// What started it: `push`, `workflow_dispatch`, ...
    pub event: String,
    /// The branch or tag the run was started for.
    pub reference: Option<String>,
    pub state: State,
    pub url: String,
}

/// Where a run stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    /// Queued, waiting or running.
    Pending,
    Success,
    /// Ended without running anything that could fail: skipped, neutral.
    Skipped,
    Cancelled,
    /// Failed, timed out, or could not start.
    Failure,
}

impl State {
    pub fn done(self) -> bool {
        self != State::Pending
    }

    /// Ended in a way that does not prove the commit good.
    pub fn red(self) -> bool {
        matches!(self, State::Failure | State::Cancelled)
    }

    pub fn word(self) -> &'static str {
        match self {
            State::Pending => "running",
            State::Success => "passed",
            State::Skipped => "skipped",
            State::Cancelled => "cancelled",
            State::Failure => "failed",
        }
    }
}

/// A release page for a tag, as the forge shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub draft: bool,
    /// The names of the files attached to it.
    pub assets: Vec<String>,
    pub url: String,
}

/// What the run and `furca push --wait-ci` ask of a forge.
pub trait Forge {
    /// Every workflow run on the commit `sha`, the newest run of each
    /// workflow, event and reference only.
    fn runs(&self, sha: &str) -> Result<Vec<Run>, String>;

    /// The release page of `tag`; `None` when there is none.
    fn page(&self, tag: &str) -> Result<Option<Page>, String>;

    /// Where a person sees the repository's runs.
    fn actions_url(&self) -> String;

    /// How often a wait may ask this forge.
    fn pace(&self) -> Pace;
}

/// A repository on a forge, read from a remote's address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    pub provider: Provider,
    /// The forge's web address: `https://github.com`.
    pub web: String,
    pub owner: String,
    pub repo: String,
}

impl Place {
    /// Reads a remote's address - `https://host/owner/repo(.git)`,
    /// `git@host:owner/repo.git`, `ssh://git@host:port/owner/repo` - as a
    /// repository on a forge. `provider` is what `release.toml` says; without
    /// it, github.com is GitHub and any other host is Forgejo.
    ///
    /// An address over SSH says nothing about the web: the forge is taken to
    /// answer HTTPS on the same host.
    pub fn from_url(url: &str, provider: Option<Provider>) -> Option<Place> {
        let (scheme, rest) = match url.split_once("://") {
            Some((scheme, rest)) => (scheme, rest),
            None => ("scp", url),
        };
        let (host, path) = match scheme {
            "http" | "https" | "ssh" | "git" => rest.split_once('/')?,
            // `user@host:owner/repo.git`
            "scp" => rest.split_once(':')?,
            _ => return None,
        };
        let host = host.rsplit('@').next()?;
        let host = match scheme {
            "http" | "https" => host.to_owned(),
            // An SSH port is not the web's port.
            _ => host.split(':').next()?.to_owned(),
        };
        let web_scheme = if scheme == "http" { "http" } else { "https" };
        let path = path.trim_matches('/').trim_end_matches(".git");
        let (owner, repo) = path.rsplit_once('/')?;
        if host.is_empty() || owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }
        let provider = provider.unwrap_or(if host.eq_ignore_ascii_case("github.com") {
            Provider::Github
        } else {
            Provider::Forgejo
        });
        Some(Place {
            provider,
            web: format!("{web_scheme}://{host}"),
            owner: owner.to_owned(),
            repo: repo.to_owned(),
        })
    }

    fn api(&self) -> String {
        match self.provider {
            Provider::Github if self.web == "https://github.com" => {
                "https://api.github.com".to_owned()
            }
            Provider::Github => format!("{}/api/v3", self.web),
            Provider::Forgejo => format!("{}/api/v1", self.web),
        }
    }
}

/// A forge asked over HTTPS.
pub struct Http {
    place: Place,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Http {
    /// A client for `place`, with the token the environment offers.
    pub fn new(place: Place) -> Http {
        let token = token(place.provider, &place.web);
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!(
                "furca/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/lacodda/furca)"
            ))
            .http_status_as_error(false)
            .build();
        Http {
            place,
            token,
            agent: config.into(),
        }
    }

    pub fn place(&self) -> &Place {
        &self.place
    }

    /// Whether the forge is asked with a token: without one GitHub allows
    /// sixty questions an hour, so a wait asks less often.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    fn get(&self, path: &str) -> Result<Option<serde_json::Value>, String> {
        let url = format!("{}{path}", self.place.api());
        let mut request = self.agent.get(&url).header("Accept", "application/json");
        if self.place.provider == Provider::Github {
            request = request.header("X-GitHub-Api-Version", "2022-11-28");
        }
        if let Some(token) = &self.token {
            let value = match self.place.provider {
                Provider::Github => format!("Bearer {token}"),
                Provider::Forgejo => format!("token {token}"),
            };
            request = request.header("Authorization", value.as_str());
        }
        let label = self.place.provider.label();
        let mut response = request
            .call()
            .map_err(|e| format!("{label} did not answer: {e}"))?;
        match response.status().as_u16() {
            200 => response
                .body_mut()
                .read_json()
                .map(Some)
                .map_err(|e| format!("{label} sent what is not JSON: {e}")),
            404 => Ok(None),
            403 | 429 if self.token.is_none() => Err(format!(
                "{label} refused an anonymous question ({}); give it a token",
                response.status().as_u16()
            )),
            status => Err(format!("{label} answered {status} for {path}")),
        }
    }

    fn repo_path(&self) -> String {
        format!("/repos/{}/{}", self.place.owner, self.place.repo)
    }
}

impl Forge for Http {
    fn runs(&self, sha: &str) -> Result<Vec<Run>, String> {
        let path = match self.place.provider {
            Provider::Github => format!(
                "{}/actions/runs?head_sha={sha}&per_page=100",
                self.repo_path()
            ),
            Provider::Forgejo => {
                format!("{}/actions/runs?head_sha={sha}&limit=50", self.repo_path())
            }
        };
        let body = self.get(&path)?.ok_or_else(|| {
            format!(
                "{} does not show the runs of {}/{}: is it there, and can the token read it?",
                self.place.provider.label(),
                self.place.owner,
                self.place.repo
            )
        })?;
        let runs = match self.place.provider {
            Provider::Github => github_runs(&body),
            Provider::Forgejo => forgejo_runs(&body, sha),
        };
        Ok(newest(runs))
    }

    fn page(&self, tag: &str) -> Result<Option<Page>, String> {
        let Some(body) = self.get(&format!("{}/releases/tags/{tag}", self.repo_path()))? else {
            return Ok(None);
        };
        Ok(Some(Page {
            draft: body.get("draft").and_then(|d| d.as_bool()).unwrap_or(false),
            assets: body
                .get("assets")
                .and_then(|a| a.as_array())
                .into_iter()
                .flatten()
                .filter_map(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_owned))
                .collect(),
            url: text(&body, "html_url"),
        }))
    }

    fn pace(&self) -> Pace {
        Pace::for_forge(self.has_token())
    }

    fn actions_url(&self) -> String {
        format!(
            "{}/{}/{}/actions",
            self.place.web, self.place.owner, self.place.repo
        )
    }
}

fn text(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_owned()
}

/// `workflow_runs` of GitHub's `actions/runs`. A re-run keeps its id and
/// moves its attempt on; the state is the latest attempt's.
fn github_runs(body: &serde_json::Value) -> Vec<(String, Run)> {
    let mut out = Vec::new();
    for run in body
        .get("workflow_runs")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
    {
        let path = text(run, "path");
        let workflow = path.rsplit('/').next().unwrap_or(&path).to_owned();
        // A dynamic workflow (`dynamic/...`) has an `@` and a ref in its path.
        let workflow = workflow.split('@').next().unwrap_or_default().to_owned();
        let state = match run.get("status").and_then(|s| s.as_str()) {
            Some("completed") => match run.get("conclusion").and_then(|c| c.as_str()) {
                Some("success") => State::Success,
                Some("skipped" | "neutral") => State::Skipped,
                Some("cancelled") => State::Cancelled,
                _ => State::Failure,
            },
            _ => State::Pending,
        };
        out.push((
            text(run, "created_at"),
            Run {
                id: run.get("id").and_then(|i| i.as_u64()).unwrap_or_default(),
                workflow,
                name: text(run, "name"),
                event: text(run, "event"),
                reference: run
                    .get("head_branch")
                    .and_then(|b| b.as_str())
                    .map(str::to_owned),
                state,
                url: text(run, "html_url"),
            },
        ));
    }
    out
}

/// `workflow_runs` of Forgejo's `actions/runs`. Forgejo keeps one status
/// for both the progress and the outcome.
fn forgejo_runs(body: &serde_json::Value, sha: &str) -> Vec<(String, Run)> {
    let mut out = Vec::new();
    for run in body
        .get("workflow_runs")
        .and_then(|r| r.as_array())
        .into_iter()
        .flatten()
    {
        // An older Forgejo ignores `head_sha`; the filter is ours as well.
        if run.get("commit_sha").and_then(|c| c.as_str()) != Some(sha) {
            continue;
        }
        let state = match run.get("status").and_then(|s| s.as_str()) {
            Some("success") => State::Success,
            Some("skipped") => State::Skipped,
            Some("cancelled") => State::Cancelled,
            Some("failure") => State::Failure,
            _ => State::Pending,
        };
        let workflow = text(run, "workflow_id");
        let reference = run
            .get("prettyref")
            .and_then(|r| r.as_str())
            .map(str::to_owned);
        out.push((
            text(run, "created"),
            Run {
                id: run.get("id").and_then(|i| i.as_u64()).unwrap_or_default(),
                name: workflow
                    .trim_end_matches(".yml")
                    .trim_end_matches(".yaml")
                    .to_owned(),
                workflow,
                event: text(run, "event"),
                reference,
                state,
                url: text(run, "html_url"),
            },
        ));
    }
    out
}

/// The newest run of each workflow, event and reference, in workflow order.
fn newest(mut runs: Vec<(String, Run)>) -> Vec<Run> {
    // Timestamps in one format sort as text; the id breaks a tie.
    runs.sort_by(|a, b| (&b.0, b.1.id).cmp(&(&a.0, a.1.id)));
    let mut out: Vec<Run> = Vec::new();
    for (_, run) in runs {
        if !out.iter().any(|seen| {
            seen.workflow == run.workflow
                && seen.event == run.event
                && seen.reference == run.reference
        }) {
            out.push(run);
        }
    }
    out.sort_by(|a, b| (&a.workflow, &a.event).cmp(&(&b.workflow, &b.event)));
    out
}

/// The token for a forge, from the environment; for github.com also from
/// the `gh` CLI when it is signed in.
fn token(provider: Provider, web: &str) -> Option<String> {
    let from_env = |names: &[&str]| {
        names
            .iter()
            .filter_map(|n| std::env::var(n).ok())
            .map(|t| t.trim().to_owned())
            .find(|t| !t.is_empty())
    };
    match provider {
        Provider::Forgejo => from_env(&["FORGEJO_TOKEN"]),
        Provider::Github => from_env(&["GH_TOKEN", "GITHUB_TOKEN"]).or_else(|| {
            if web != "https://github.com" {
                return None;
            }
            let output = Command::new("gh").args(["auth", "token"]).output().ok()?;
            let token = String::from_utf8(output.stdout).ok()?.trim().to_owned();
            (output.status.success() && !token.is_empty()).then_some(token)
        }),
    }
}

/// How long and how often a wait asks.
#[derive(Debug, Clone, Copy)]
pub struct Pace {
    /// Between two questions.
    pub every: Duration,
    /// How long the runs may take to appear after a push.
    pub appear: Duration,
    /// How long they may take to finish.
    pub limit: Duration,
}

impl Pace {
    /// The pace for a forge: every 15 seconds with a token, every minute
    /// without one (GitHub's anonymous limit is sixty an hour); five minutes
    /// for the runs to appear, two hours for them to finish.
    pub fn for_forge(has_token: bool) -> Pace {
        Pace {
            every: Duration::from_secs(if has_token { 15 } else { 60 }),
            appear: Duration::from_secs(5 * 60),
            limit: Duration::from_secs(2 * 60 * 60),
        }
    }
}

/// How a wait ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "outcome")]
pub enum Waited {
    /// Every expected workflow ran, and none failed.
    Green { runs: Vec<Run> },
    /// A run failed or was cancelled; the others may still be running.
    Red { runs: Vec<Run> },
    /// An expected workflow never appeared on the commit.
    Missing {
        runs: Vec<Run>,
        missing: Vec<String>,
    },
    /// The time ran out with runs still going.
    Timeout { runs: Vec<Run> },
}

impl Waited {
    pub fn runs(&self) -> &[Run] {
        match self {
            Waited::Green { runs }
            | Waited::Red { runs }
            | Waited::Missing { runs, .. }
            | Waited::Timeout { runs } => runs,
        }
    }
}

/// Waits for the runs on `sha` that `keep` selects: until every one has
/// finished or one has failed. `expect` names the workflows (file names)
/// that must be among them; with none named, at least one run must appear.
///
/// `sleep` is called between questions, and `seen` each time the runs'
/// states change, for a progress line.
pub fn wait(
    forge: &dyn Forge,
    sha: &str,
    keep: &dyn Fn(&Run) -> bool,
    expect: &[&str],
    pace: Pace,
    sleep: &dyn Fn(Duration),
    seen: &mut dyn FnMut(&[Run]),
) -> Result<Waited, String> {
    let mut waited = Duration::ZERO;
    let mut last: Option<Vec<(String, State)>> = None;
    loop {
        let runs: Vec<Run> = forge.runs(sha)?.into_iter().filter(|r| keep(r)).collect();
        let states: Vec<(String, State)> =
            runs.iter().map(|r| (r.workflow.clone(), r.state)).collect();
        if last.as_ref() != Some(&states) {
            seen(&runs);
            last = Some(states);
        }

        let missing: Vec<String> = if expect.is_empty() {
            if runs.is_empty() {
                vec!["any workflow".to_owned()]
            } else {
                Vec::new()
            }
        } else {
            expect
                .iter()
                .filter(|w| !runs.iter().any(|r| r.workflow == **w))
                .map(|w| (*w).to_owned())
                .collect()
        };
        if runs.iter().any(|r| r.state.red()) {
            return Ok(Waited::Red { runs });
        }
        if missing.is_empty() && runs.iter().all(|r| r.state.done()) {
            return Ok(Waited::Green { runs });
        }
        if !missing.is_empty() && waited >= pace.appear {
            return Ok(Waited::Missing { runs, missing });
        }
        if waited >= pace.limit {
            return Ok(Waited::Timeout { runs });
        }
        sleep(pace.every);
        waited += pace.every;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_remote_address_names_its_forge() {
        let github = Place::from_url("https://github.com/lacodda/furca.git", None).expect("reads");
        assert_eq!(
            (
                github.provider,
                github.web.as_str(),
                github.owner.as_str(),
                github.repo.as_str()
            ),
            (Provider::Github, "https://github.com", "lacodda", "furca")
        );
        assert_eq!(github.api(), "https://api.github.com");
        assert_eq!(
            Place::from_url("git@github.com:lacodda/furca.git", None),
            Some(github.clone())
        );
        assert_eq!(
            Place::from_url(
                "https://x-access-token:secret@github.com/lacodda/furca",
                None
            ),
            Some(github)
        );

        let forgejo = Place::from_url("http://forge.local:8080/team/app.git", None).expect("reads");
        assert_eq!(
            (forgejo.provider, forgejo.web.as_str(), forgejo.api()),
            (
                Provider::Forgejo,
                "http://forge.local:8080",
                "http://forge.local:8080/api/v1".to_owned()
            )
        );
        let ssh = Place::from_url(
            "ssh://git@forge.local:2222/team/app.git",
            Some(Provider::Forgejo),
        )
        .expect("reads");
        assert_eq!(ssh.web, "https://forge.local");

        assert_eq!(Place::from_url("/srv/git/app.git", None), None);
        assert_eq!(Place::from_url("https://github.com/lacodda", None), None);
    }

    #[test]
    fn github_runs_read_the_newest_of_each_workflow() {
        let body = serde_json::json!({ "workflow_runs": [
            { "id": 3, "name": "CI", "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
              "status": "completed", "conclusion": "success", "html_url": "u3", "created_at": "2026-10-11T10:05:00Z" },
            { "id": 2, "name": "CI", "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
              "status": "completed", "conclusion": "failure", "html_url": "u2", "created_at": "2026-10-11T10:00:00Z" },
            { "id": 4, "name": "Publish", "path": ".github/workflows/publish.yml", "event": "push", "head_branch": "v1.0.0",
              "status": "in_progress", "conclusion": null, "html_url": "u4", "created_at": "2026-10-11T10:06:00Z" },
            { "id": 5, "name": "Release", "path": ".github/workflows/release.yml", "event": "push", "head_branch": "v1.0.0",
              "status": "completed", "conclusion": "startup_failure", "html_url": "u5", "created_at": "2026-10-11T10:06:00Z" }
        ]});
        let runs = newest(github_runs(&body));
        let seen: Vec<(&str, State)> = runs
            .iter()
            .map(|r| (r.workflow.as_str(), r.state))
            .collect();
        assert_eq!(
            seen,
            [
                ("ci.yml", State::Success),
                ("publish.yml", State::Pending),
                ("release.yml", State::Failure)
            ]
        );
        assert_eq!(runs[1].reference.as_deref(), Some("v1.0.0"));
    }

    #[test]
    fn forgejo_runs_read_one_status_for_progress_and_outcome() {
        let body = serde_json::json!({ "total_count": 3, "workflow_runs": [
            { "id": 7, "workflow_id": "ci.yml", "event": "push", "prettyref": "main", "commit_sha": "abc",
              "status": "running", "html_url": "f7", "created": "2026-10-11T10:00:00Z" },
            { "id": 8, "workflow_id": "publish.yml", "event": "push", "prettyref": "v1.0.0", "commit_sha": "abc",
              "status": "success", "html_url": "f8", "created": "2026-10-11T10:01:00Z" },
            { "id": 9, "workflow_id": "ci.yml", "event": "push", "prettyref": "main", "commit_sha": "other",
              "status": "failure", "html_url": "f9", "created": "2026-10-11T10:02:00Z" }
        ]});
        let runs = newest(forgejo_runs(&body, "abc"));
        let seen: Vec<(&str, State)> = runs
            .iter()
            .map(|r| (r.workflow.as_str(), r.state))
            .collect();
        assert_eq!(
            seen,
            [("ci.yml", State::Pending), ("publish.yml", State::Success)]
        );
        assert_eq!(runs[0].name, "ci");
    }

    /// A forge that answers from a script, one answer per question.
    struct Scripted(RefCell<Vec<Vec<Run>>>);

    impl Forge for Scripted {
        fn runs(&self, _sha: &str) -> Result<Vec<Run>, String> {
            let mut answers = self.0.borrow_mut();
            Ok(if answers.len() > 1 {
                answers.remove(0)
            } else {
                answers[0].clone()
            })
        }
        fn page(&self, _tag: &str) -> Result<Option<Page>, String> {
            Ok(None)
        }
        fn actions_url(&self) -> String {
            String::new()
        }
        fn pace(&self) -> Pace {
            PACE
        }
    }

    fn run(workflow: &str, state: State) -> Run {
        Run {
            id: 1,
            workflow: workflow.to_owned(),
            name: workflow.to_owned(),
            event: "push".to_owned(),
            reference: Some("main".to_owned()),
            state,
            url: String::new(),
        }
    }

    const PACE: Pace = Pace {
        every: Duration::from_secs(10),
        appear: Duration::from_secs(30),
        limit: Duration::from_secs(100),
    };

    fn waited(answers: Vec<Vec<Run>>, expect: &[&str]) -> (Waited, usize) {
        let forge = Scripted(RefCell::new(answers));
        let sleeps = RefCell::new(0);
        let outcome = wait(
            &forge,
            "abc",
            &|_| true,
            expect,
            PACE,
            &|_| *sleeps.borrow_mut() += 1,
            &mut |_| {},
        )
        .expect("waits");
        (outcome, sleeps.into_inner())
    }

    #[test]
    fn a_wait_ends_green_when_every_expected_run_has_passed() {
        let (outcome, sleeps) = waited(
            vec![
                vec![],
                vec![run("ci.yml", State::Pending)],
                vec![
                    run("ci.yml", State::Success),
                    run("lint.yml", State::Skipped),
                ],
            ],
            &["ci.yml"],
        );
        assert!(matches!(outcome, Waited::Green { .. }), "{outcome:?}");
        assert_eq!(sleeps, 2);
    }

    #[test]
    fn a_wait_stops_at_the_first_failure() {
        let (outcome, _) = waited(
            vec![vec![
                run("ci.yml", State::Pending),
                run("docs.yml", State::Failure),
            ]],
            &["ci.yml"],
        );
        assert!(matches!(outcome, Waited::Red { .. }), "{outcome:?}");
    }

    #[test]
    fn a_workflow_that_never_appears_is_missing_and_a_slow_one_times_out() {
        let (outcome, sleeps) = waited(vec![vec![run("docs.yml", State::Success)]], &["ci.yml"]);
        assert_eq!(
            outcome,
            Waited::Missing {
                runs: vec![run("docs.yml", State::Success)],
                missing: vec!["ci.yml".to_owned()]
            }
        );
        assert_eq!(sleeps, 3, "appear is 30 seconds at 10 a question");

        let (outcome, sleeps) = waited(vec![vec![run("ci.yml", State::Pending)]], &["ci.yml"]);
        assert!(matches!(outcome, Waited::Timeout { .. }), "{outcome:?}");
        assert_eq!(sleeps, 10);

        let (outcome, _) = waited(vec![vec![]], &[]);
        assert!(
            matches!(outcome, Waited::Missing { .. }),
            "no run at all: {outcome:?}"
        );
    }
}
