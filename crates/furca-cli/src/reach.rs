//! The commands that talk to a remote - `fetch`, `pull`, `push` - and
//! `remote`, which keeps the list of them. Each is one call into the core,
//! which has the system `git` do it: credentials are its credential helper's
//! and ssh-agent's, and furca never asks for one.
//!
//! How far a run has got goes to standard error on one line rewritten in
//! place, when standard error is a terminal; the answer goes to standard
//! output, as text or as `--json`.

use std::io::{IsTerminal, Write};

use furca_core::{Fetched, Moved, Progress, Pulled, Pushed, Reconcile, Remote, Remotes};
use furca_release::hosting::{self, Waited};
use serde::Serialize;

use crate::{
    Answer, Common, RemoteCommand, emit, forge, not_a_forge, open, provider_of, say, short, states,
    waited_lines,
};

/// A run's progress, on one line of standard error rewritten as it goes.
struct Ticker {
    step: &'static str,
    on: bool,
    /// How wide the last line was, which the next one covers.
    wide: usize,
}

impl Ticker {
    fn new(step: &'static str) -> Ticker {
        Ticker {
            step,
            on: std::io::stderr().is_terminal(),
            wide: 0,
        }
    }

    fn hear(&mut self, remote: &str, progress: &Progress) {
        if !self.on {
            return;
        }
        let count = progress.percent().map_or_else(
            || progress.done.to_string(),
            |percent| format!("{percent}%"),
        );
        let line = format!(
            "  {:<9}  {remote}{}{} {count}",
            self.step,
            if remote.is_empty() { "" } else { "  " },
            progress.phase
        );
        let wide = self.wide.max(line.chars().count());
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "\r{line:<wide$}");
        let _ = err.flush();
        self.wide = wide;
    }

    /// Takes the line away, for what is said after it.
    fn clear(&mut self) {
        if self.wide > 0 {
            let wide = self.wide;
            let mut err = std::io::stderr().lock();
            let _ = write!(err, "\r{:wide$}\r", "");
            let _ = err.flush();
            self.wide = 0;
        }
    }
}

pub(crate) fn fetch(
    common: &Common,
    remote: Option<String>,
    all: bool,
    prune: bool,
) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let from = match (remote, all) {
        (Some(name), _) => Remotes::Named(name),
        (None, true) => Remotes::All,
        (None, false) => Remotes::Default,
    };
    let mut ticker = Ticker::new("fetch");
    let fetched = repo
        .git()
        .fetch(&from, prune, &mut |progress| ticker.hear("", progress));
    ticker.clear();
    let fetched = fetched.map_err(|e| e.to_string())?;
    emit(common.json, &fetched, fetched_text)
}

pub(crate) fn pull(common: &Common, how: Reconcile) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let mut ticker = Ticker::new("pull");
    let pulled = repo
        .git()
        .pull(how, &mut |progress| ticker.hear("", progress));
    ticker.clear();
    let pulled = pulled.map_err(|e| e.to_string())?;
    emit(common.json, &pulled, pulled_text)
}

/// What `push` did, for `--json`.
#[derive(Serialize)]
struct Pushes {
    /// The branch pushed.
    branch: String,
    /// Each remote it reached, in the order it was pushed.
    pushed: Vec<Pushed>,
    /// Each remote it did not, and git's words for why.
    failed: Vec<NotPushed>,
    /// How the CI runs the push started ended; `null` without `--wait-ci`.
    ci: Option<Waited>,
}

#[derive(Serialize)]
struct NotPushed {
    remote: String,
    to: String,
    error: String,
}

pub(crate) fn push(
    common: &Common,
    to: &Remotes,
    force: bool,
    wait_ci: bool,
) -> Result<Answer, String> {
    let repo = open(&common.repo)?;
    let destinations = repo.destinations(to).map_err(|e| e.to_string())?;
    let branch = destinations
        .first()
        .map(|d| d.branch.clone())
        .unwrap_or_default();
    let mut pushes = Pushes {
        branch,
        pushed: Vec::new(),
        failed: Vec::new(),
        ci: None,
    };
    let mut ticker = Ticker::new("push");
    for destination in &destinations {
        let done = repo.git().push_branch(destination, force, &mut |progress| {
            ticker.hear(&destination.remote, progress)
        });
        ticker.clear();
        match done {
            Ok(pushed) => {
                say(
                    "push",
                    &format!(
                        "{} to {} {}",
                        short(&pushed.commit),
                        pushed.remote,
                        pushed.to
                    ),
                );
                pushes.pushed.push(pushed);
            }
            Err(error) => {
                say("push", &format!("not to {}", destination.remote));
                pushes.failed.push(NotPushed {
                    remote: destination.remote.clone(),
                    to: destination.to.clone(),
                    error: error.to_string(),
                });
            }
        }
    }

    if wait_ci {
        // CI is waited for where it is promised: the remote named, or the one
        // the branch goes to by default - not each mirror of it.
        let ci_remote = match to {
            Remotes::Named(name) => Some(name.clone()),
            Remotes::Default | Remotes::All => repo
                .destinations(&Remotes::Default)
                .ok()
                .and_then(|d| d.into_iter().next())
                .map(|d| d.remote),
        };
        if let Some(pushed) = pushes
            .pushed
            .iter()
            .find(|p| Some(&p.remote) == ci_remote.as_ref())
        {
            let url = repo
                .remote_url(&pushed.remote)
                .map_err(|e| e.to_string())?
                .unwrap_or_default();
            let forge =
                forge(&url, provider_of(&repo)).ok_or_else(|| not_a_forge(&pushed.remote, &url))?;
            let there = pushed.to.trim_start_matches("refs/heads/").to_owned();
            let waited = hosting::wait(
                forge.as_ref(),
                &pushed.commit,
                &|run| run.reference.as_deref() == Some(there.as_str()),
                &[],
                forge.pace(),
                &std::thread::sleep,
                &mut |runs| say("ci", &states(runs)),
            )?;
            pushes.ci = Some(waited);
        }
    }

    emit(common.json, &pushes, pushes_text)?;
    if !pushes.failed.is_empty() {
        let names: Vec<&str> = pushes.failed.iter().map(|f| f.remote.as_str()).collect();
        return Err(format!("not pushed to {}", names.join(", ")));
    }
    Ok(match &pushes.ci {
        None | Some(Waited::Green { .. }) => Answer::Yes,
        Some(_) => Answer::No,
    })
}

pub(crate) fn remote(command: Option<RemoteCommand>, common: &Common) -> Result<Answer, String> {
    let Some(command) = command else {
        return list(common);
    };
    let (common, said) = match command {
        RemoteCommand::Add { common, name, url } => {
            open(&common.repo)?
                .git()
                .remote_add(&name, &url)
                .map_err(|e| e.to_string())?;
            let said = format!("Added {name}; `furca fetch {name}` brings its branches.");
            (common, said)
        }
        RemoteCommand::Rename { common, from, to } => {
            open(&common.repo)?
                .git()
                .remote_rename(&from, &to)
                .map_err(|e| e.to_string())?;
            (common, format!("Renamed {from} to {to}."))
        }
        RemoteCommand::SetUrl { common, name, url } => {
            open(&common.repo)?
                .git()
                .remote_set_url(&name, &url)
                .map_err(|e| e.to_string())?;
            (common, format!("Pointed {name} elsewhere."))
        }
        RemoteCommand::Remove { common, name } => {
            open(&common.repo)?
                .git()
                .remote_remove(&name)
                .map_err(|e| e.to_string())?;
            let said = format!("Removed {name} and its remote-tracking branches.");
            (common, said)
        }
        RemoteCommand::Prune { common, name } => {
            let pruned = open(&common.repo)?
                .git()
                .remote_prune(&name)
                .map_err(|e| e.to_string())?;
            return emit(common.json, &pruned, pruned_text);
        }
    };
    if !common.json {
        println!("{said}");
    }
    // The list as it is now, read afresh: a handle reads the configuration
    // when it is opened.
    list(&common)
}

fn list(common: &Common) -> Result<Answer, String> {
    let remotes = open(&common.repo)?.remotes().map_err(|e| e.to_string())?;
    emit(common.json, remotes.as_slice(), remotes_text)
}

fn remotes_text(remotes: &[Remote]) -> String {
    if remotes.is_empty() {
        return "No remotes".to_owned();
    }
    let width = remotes.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for remote in remotes {
        out.push(format!(
            "{:<width$}  {}",
            remote.name,
            remote.url.as_deref().unwrap_or("(no address)")
        ));
        if let Some(push) = &remote.push_url {
            out.push(format!("{:<width$}  pushes to {push}", ""));
        }
    }
    out.join("\n")
}

/// A ref as a person reads it: `origin/main`, `tag v1.0.0`.
fn ref_name(name: &str) -> String {
    if let Some(tag) = name.strip_prefix("refs/tags/") {
        format!("tag {tag}")
    } else {
        name.strip_prefix("refs/remotes/")
            .unwrap_or(name)
            .to_owned()
    }
}

fn moved_lines(moved: &[Moved]) -> Vec<String> {
    let names: Vec<String> = moved.iter().map(|m| ref_name(&m.name)).collect();
    let width = names.iter().map(String::len).max().unwrap_or(0);
    moved
        .iter()
        .zip(&names)
        .map(|(m, name)| {
            let how = match (&m.from, &m.to) {
                (Some(from), Some(to)) => format!("{}..{}", short(from), short(to)),
                (None, Some(to)) => format!("new at {}", short(to)),
                (Some(_), None) => "gone".to_owned(),
                (None, None) => String::new(),
            };
            format!("  {name:<width$}  {how}")
        })
        .collect()
}

fn fetched_text(fetched: &Fetched) -> String {
    let remotes = fetched.remotes.join(", ");
    if fetched.moved.is_empty() {
        return format!("Fetched {remotes}: nothing moved");
    }
    let mut out = vec![format!(
        "Fetched {remotes}: {} moved",
        refs(fetched.moved.len())
    )];
    out.extend(moved_lines(&fetched.moved));
    out.join("\n")
}

fn pruned_text(pruned: &Fetched) -> String {
    let remote = pruned.remotes.join(", ");
    if pruned.moved.is_empty() {
        return format!("Nothing to prune on {remote}");
    }
    let mut out = vec![format!("Pruned {} of {remote}", refs(pruned.moved.len()))];
    out.extend(moved_lines(&pruned.moved));
    out.join("\n")
}

fn refs(count: usize) -> String {
    if count == 1 {
        "1 ref".to_owned()
    } else {
        format!("{count} refs")
    }
}

fn pulled_text(pulled: &Pulled) -> String {
    match (&pulled.from, &pulled.to) {
        (from, to) if from == to => {
            format!("{} is up to date with {}", pulled.branch, pulled.upstream)
        }
        (Some(from), Some(to)) => format!(
            "Pulled {} into {}: {}..{}",
            pulled.upstream,
            pulled.branch,
            short(from),
            short(to)
        ),
        (_, to) => format!(
            "Pulled {} into {}: now at {}",
            pulled.upstream,
            pulled.branch,
            to.as_deref().map(short).unwrap_or("nothing")
        ),
    }
}

fn pushes_text(pushes: &Pushes) -> String {
    let mut out = Vec::new();
    for pushed in &pushes.pushed {
        let mut line = format!(
            "Pushed {} to {} {}",
            short(&pushed.commit),
            pushed.remote,
            pushed.to.trim_start_matches("refs/heads/")
        );
        if pushed.forced {
            line.push_str(", with a lease");
        }
        if pushed.upstream_set {
            line.push_str(&format!("; {} follows it now", pushed.branch));
        }
        out.push(line);
    }
    for failed in &pushes.failed {
        out.push(format!("Not pushed to {}: {}", failed.remote, failed.error));
    }
    if let Some(ci) = &pushes.ci {
        out.extend(waited_lines(ci));
    }
    out.join("\n")
}
