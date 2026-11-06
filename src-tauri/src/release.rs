//! The release panel: `furca-release` behind the window, as the CLI has it.
//!
//! The plan is asked for and answered. A run outlives any one command, so it has
//! a thread of its own and the window hears it on the `release` event: each step
//! as it begins and ends, what the programs print, and how it ended. One run per
//! repository in this window. A window closed mid-run stops it where it is, and
//! the checkpoint knows where that was (ADR 0007).
//!
//! Nothing is decided here. What may run, what stops and what a stop means are
//! the engine's, so the window and the CLI cannot disagree.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use furca_release::hosting::forge_at;
use furca_release::{
    Error, Http, Local, Output, Plan, Provider, Release, Report, Rigger, Runner, STEPS, Sources,
    Told, UnderWay,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::off_thread;

/// The event a run is heard on.
pub const EVENT: &str = "release";

/// How long what a run says is gathered before it goes out: a compiler prints
/// faster than a window should be told.
const GATHER: Duration = Duration::from_millis(60);

/// What the panel starts from.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ReleaseState {
    /// The repository has a `release.toml`. Without one there is no panel.
    pub configured: bool,
    /// The steps of a run, in order, as the engine names them.
    pub steps: Vec<&'static str>,
    /// Why what is there cannot be read.
    pub broken: Option<String>,
    /// The run that has not finished, as its checkpoint has it.
    pub under_way: Option<UnderWay>,
    /// A run is going in this window now.
    pub running: bool,
}

/// What the window hears of a run.
#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct ReleaseHeard {
    /// The repository, as its tab knows it.
    pub path: String,
    /// In the order it happened.
    pub heard: Vec<Heard>,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Heard {
    /// What the run says of its steps.
    Told { told: Told },
    /// A line a program printed, in the step going on.
    Printed { line: String },
    /// The run ended, released or stopped: the report says which.
    Finished { report: Report },
    /// The run could not go on at all: what the engine relies on did not work.
    Failed { reason: String },
}

/// The repositories a run is going in.
#[derive(Default)]
pub struct Releases(Mutex<HashSet<String>>);

impl Releases {
    fn is_running(&self, path: &str) -> bool {
        self.0.lock().is_ok_and(|running| running.contains(path))
    }
}

/// What the panel of the repository at `path` starts from.
#[tauri::command]
pub async fn release_state(
    releases: State<'_, Releases>,
    path: String,
) -> Result<ReleaseState, String> {
    let running = releases.is_running(&path);
    off_thread(move || Ok(state_of(&path, running))).await
}

fn state_of(path: &str, running: bool) -> ReleaseState {
    let opened = match Release::open(path) {
        Ok(release) => release,
        Err(Error::NoConfig(_) | Error::Bare(_)) => {
            return ReleaseState {
                configured: false,
                steps: STEPS.to_vec(),
                broken: None,
                under_way: None,
                running: false,
            };
        }
        Err(error) => return broken(error.to_string(), running),
    };
    match opened.under_way() {
        Ok(under_way) => ReleaseState {
            configured: true,
            steps: STEPS.to_vec(),
            broken: None,
            under_way,
            running,
        },
        Err(error) => broken(error.to_string(), running),
    }
}

fn broken(reason: String, running: bool) -> ReleaseState {
    ReleaseState {
        configured: true,
        steps: STEPS.to_vec(),
        broken: Some(reason),
        under_way: None,
        running,
    }
}

/// The next release of the repository at `path`, and what stands in its way. It
/// asks the registries and the record and changes nothing.
#[tauri::command]
pub async fn release_plan(path: String) -> Result<Plan, String> {
    off_thread(move || {
        let release = Release::open(&path).map_err(|e| e.to_string())?;
        release
            .plan(&Sources {
                registries: Some(&Http::default()),
                record: Some(&Rigger),
                now: SystemTime::now(),
            })
            .map_err(|e| e.to_string())
    })
    .await
}

/// Starts a run of the repository at `path`, or carries on the one that stopped
/// with `resume`, and returns at once: the run is heard on [`EVENT`].
#[tauri::command]
pub async fn release_run(
    app: AppHandle,
    releases: State<'_, Releases>,
    path: String,
    resume: bool,
) -> Result<(), String> {
    {
        let mut running = releases
            .0
            .lock()
            .map_err(|_| "the list of running releases is poisoned")?;
        if !running.insert(path.clone()) {
            return Err("a release is running here already".to_owned());
        }
    }
    let thread = {
        let (app, path) = (app.clone(), path.clone());
        std::thread::Builder::new()
            .name("furca-release".to_owned())
            .spawn(move || {
                run_and_tell(&app, &path, resume);
                if let Ok(mut running) = app.state::<Releases>().0.lock() {
                    running.remove(&path);
                }
            })
    };
    if let Err(error) = thread {
        if let Ok(mut running) = releases.0.lock() {
            running.remove(&path);
        }
        return Err(format!("could not start the release: {error}"));
    }
    Ok(())
}

/// Runs and tells the window, to the last word. A panic is told like any other
/// end, or the panel would wait on a run that is gone.
fn run_and_tell(app: &AppHandle, path: &str, resume: bool) {
    let (tell, heard) = mpsc::channel();
    let forwarder = {
        let (app, path) = (app.clone(), path.to_owned());
        std::thread::spawn(move || forward(&app, &path, &heard))
    };
    let ended = catch_unwind(AssertUnwindSafe(|| run(path, resume, &tell)));
    let last = match ended {
        Ok(Ok(report)) => Heard::Finished { report },
        Ok(Err(reason)) => Heard::Failed { reason },
        Err(_) => Heard::Failed {
            reason: "the release stopped unexpectedly: its checkpoint says where".to_owned(),
        },
    };
    let _ = tell.send(last);
    drop(tell);
    let _ = forwarder.join();
}

fn run(path: &str, resume: bool, tell: &Sender<Heard>) -> Result<Report, String> {
    let release = Release::open(path).map_err(|e| e.to_string())?;
    let http = Http::default();
    let printing = tell.clone();
    let machine = Local {
        output: Output::Lines(Arc::new(move |line: &str| {
            let _ = printing.send(Heard::Printed {
                line: line.to_owned(),
            });
        })),
    };
    let forge = |url: &str, provider: Provider| forge_at(url, Some(provider));
    let mut told = |told: Told| {
        let _ = tell.send(Heard::Told { told });
    };
    let mut runner = Runner {
        registries: &http,
        web: &http,
        record: Some(&Rigger),
        forge: &forge,
        machine: &machine,
        now: &SystemTime::now,
        sleep: &std::thread::sleep,
        tell: &mut told,
    };
    if resume {
        release.resume(&mut runner)
    } else {
        release.run(&mut runner)
    }
    .map_err(|e| e.to_string())
}

/// Sends what is heard to the window, gathered, until the run has gone.
fn forward(app: &AppHandle, path: &str, heard: &Receiver<Heard>) {
    while let Ok(first) = heard.recv() {
        let gathered = gather(first, heard, Instant::now() + GATHER);
        let _ = app.emit(
            EVENT,
            ReleaseHeard {
                path: path.to_owned(),
                heard: gathered,
            },
        );
    }
}

/// `first` and everything else heard until `until`, in order.
fn gather<T>(first: T, heard: &Receiver<T>, until: Instant) -> Vec<T> {
    let mut gathered = vec![first];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        match heard.recv_timeout(left) {
            Ok(next) => gathered.push(next),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return gathered,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_heard_goes_out_gathered_and_in_order() {
        let (tell, heard) = mpsc::channel();
        for line in 2..=4 {
            tell.send(line).expect("sent");
        }
        let until = Instant::now() + Duration::from_millis(20);
        assert_eq!(gather(1, &heard, until), [1, 2, 3, 4]);
    }

    #[test]
    fn a_run_that_has_gone_ends_the_gathering_at_once() {
        let (tell, heard) = mpsc::channel();
        tell.send(2).expect("sent");
        drop(tell);
        let began = Instant::now();
        let until = began + Duration::from_secs(60);
        assert_eq!(gather(1, &heard, until), [1, 2]);
        assert!(began.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_repository_with_no_release_toml_has_no_panel() {
        let dir = tempfile::tempdir().expect("temp dir");
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .expect("git runs");
        assert!(git.success());
        let state = state_of(&dir.path().to_string_lossy(), false);
        assert!(!state.configured);
        assert_eq!(state.broken, None);
        assert!(state.under_way.is_none());
    }

    #[test]
    fn a_release_toml_that_does_not_read_says_why_rather_than_hiding_the_panel() {
        let dir = tempfile::tempdir().expect("temp dir");
        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .expect("git runs");
        assert!(git.success());
        std::fs::write(dir.path().join("release.toml"), "not = [toml").expect("write");
        let state = state_of(&dir.path().to_string_lossy(), true);
        assert!(state.configured);
        assert!(state.broken.is_some(), "{state:?}");
        assert!(state.running);
    }
}
