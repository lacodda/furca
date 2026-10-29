//! The speed budgets furca promises, measured.
//!
//! `cargo bench -p furca-core --bench budgets` builds the fixture once, times
//! each budget whose status is `measured`, writes the numbers to
//! `target/budgets.json` and exits non-zero if any promise is broken — a red
//! benchmark is a red gate.
//!
//! The promises themselves live in `benches/budgets.json`, which the docs
//! site reads too: one list, so the page cannot promise what the gate does not
//! hold. This harness measures the core's; the window's are measured by
//! `src-tauri/benches/window.rs`, which starts the built app.
//!
//! A plain harness rather than criterion: the gate needs a median compared to
//! a number, not a statistical report, and that is a few lines.

#[path = "../tests/fixture/mod.rs"]
mod fixture;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use fixture::Layout;
use furca_core::{Repository, Tips};
use serde::{Deserialize, Serialize};

const PROMISES: &str = include_str!("budgets.json");

/// Runs thrown away before measuring: the first touches the disk cache.
const WARMUP: usize = 3;
/// Runs measured; the median is what a budget is held to.
const RUNS: usize = 21;

#[derive(Deserialize)]
struct Promises {
    commits: u32,
    budgets: Vec<Promise>,
}

#[derive(Deserialize)]
struct Promise {
    name: String,
    /// Which door's harness measures it: `core` here, `window` in the app's.
    door: String,
    what: String,
    layout: String,
    /// The most it may take, in `unit`; `None` when it is measured and
    /// published but not promised.
    limit: Option<f64>,
    unit: String,
    status: String,
    /// Platforms - as `std::env::consts::OS` names them - where the budget
    /// is measured and published but not promised; its note says why.
    #[serde(default)]
    unpromised_on: Vec<String>,
}

impl Promise {
    /// The limit held on this platform, if any.
    fn limit_here(&self) -> Option<f64> {
        let here = std::env::consts::OS;
        self.limit
            .filter(|_| !self.unpromised_on.iter().any(|os| os == here))
    }
}

#[derive(Serialize)]
struct Report {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    commits: u32,
    runs: usize,
    results: Vec<Measured>,
}

#[derive(Serialize)]
struct Measured {
    name: String,
    what: String,
    layout: String,
    unit: &'static str,
    budget: Option<f64>,
    median: f64,
    p90: f64,
    within: bool,
}

/// The code a budget times, by the name `budgets.json` gives it.
fn runner(name: &str) -> fn(&Path) {
    match name {
        "graph-first-500" => first_500,
        "diff-5000-lines" => diff_5000,
        other => panic!("budgets.json measures `{other}`, which has no runner in budgets.rs"),
    }
}

fn layout(name: &str) -> Layout {
    [Layout::PackedWithGraph, Layout::Packed]
        .into_iter()
        .find(|layout| layout.name() == name)
        .unwrap_or_else(|| panic!("budgets.json names an unknown layout `{name}`"))
}

/// What the window does for the first screen of a repository, and more:
/// begin the graph of every branch and tag, lay out 500 rows of it and read
/// the commits on them.
fn first_500(path: &Path) {
    let repo = Repository::open(path).expect("open the fixture");
    let mut graph = repo.graph(Tips::All).expect("begin the graph");
    assert_eq!(graph.extend(&repo, 500).expect("lay out the graph"), 500);
    assert_eq!(graph.rows(0..500).len(), 500);
    let commits = graph.commits(&repo, 0..500).expect("read the commits");
    assert_eq!(commits.len(), 500);
}

/// What the window does when a file of a commit is chosen: read what the
/// commit changed, then both sides of the file and the hunks between them.
/// The fixture's newest commit rewrote one line in four of a long file.
fn diff_5000(path: &Path) {
    let repo = Repository::open(path).expect("open the fixture");
    let changes = repo.changes("main").expect("read what the commit changed");
    let file = changes
        .files
        .iter()
        .find(|file| file.path == fixture::LONG_FILE)
        .expect("the long file changed");
    let diff = repo.diff(file).expect("diff the long file");
    assert_eq!(diff.hunks.len(), (fixture::LONG_LINES / 4) as usize);
}

/// Median and 90th percentile of `RUNS` timed runs after `WARMUP` untimed.
fn measure(run: fn(&Path), path: &Path) -> (Duration, Duration) {
    for _ in 0..WARMUP {
        run(path);
    }
    let mut times: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            run(path);
            start.elapsed()
        })
        .collect();
    times.sort();
    (times[RUNS / 2], times[RUNS * 9 / 10])
}

fn main() {
    let promises: Promises = serde_json::from_str(PROMISES).expect("budgets.json parses");
    let ours = |p: &&Promise| p.status == "measured" && p.door == "core";
    // Checked on every run, `cargo test` included: a promise the harness
    // cannot measure is caught before anyone reads it on the docs site.
    for promise in promises.budgets.iter().filter(ours) {
        runner(&promise.name);
        layout(&promise.layout);
        assert_eq!(
            promise.unit, "ms",
            "{} times, so its unit is ms",
            promise.name
        );
    }

    // `cargo bench` passes `--bench`; `cargo test --benches` runs the target
    // without it, and a test run should not spend a minute measuring.
    if !std::env::args().any(|arg| arg == "--bench") {
        return;
    }

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let mut results = Vec::new();
    for promise in promises.budgets.iter().filter(ours) {
        let path = fixture::ensure(&root, promises.commits, layout(&promise.layout));
        let run = runner(&promise.name);
        let limit = promise
            .limit_here()
            .map(|ms| Duration::from_secs_f64(ms / 1000.0));

        let (mut median, mut p90) = measure(run, &path);
        // Known flake: a busy machine - a browser, a build, the antivirus on
        // a fresh binary - slows every run for seconds at a time. One repeat
        // after a pause separates that from a regression, which stays over.
        if limit.is_some_and(|limit| median > limit) {
            std::thread::sleep(Duration::from_secs(5));
            (median, p90) = measure(run, &path);
        }

        let measured = Measured {
            name: promise.name.clone(),
            what: promise.what.clone(),
            layout: promise.layout.clone(),
            unit: "ms",
            budget: promise.limit_here(),
            median: ms(median),
            p90: ms(p90),
            within: limit.is_none_or(|limit| median <= limit),
        };
        let verdict = match (measured.budget, measured.within) {
            (None, _) => "measured, not promised".to_owned(),
            (Some(limit), true) => format!("ok, budget {limit:.0} ms"),
            (Some(limit), false) => format!("OVER budget {limit:.0} ms"),
        };
        println!(
            "{:<18} {:<20} median {:>8.1} ms  p90 {:>8.1} ms  {verdict}",
            measured.name, measured.layout, measured.median, measured.p90,
        );
        results.push(measured);
    }

    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        commits: promises.commits,
        runs: RUNS,
        results,
    };
    let out = root
        .parent()
        .expect("CARGO_TARGET_TMPDIR sits inside the target dir")
        .join("budgets.json");
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&report).expect("serialize the report"),
    )
    .expect("write budgets.json");
    println!("wrote {}", out.display());

    let over: Vec<String> = report
        .results
        .iter()
        .filter(|m| !m.within)
        .map(|m| format!("{} on {}", m.name, m.layout))
        .collect();
    if !over.is_empty() {
        eprintln!("over budget: {}", over.join(", "));
        std::process::exit(1);
    }
}

fn ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 10_000.0).round() / 10.0
}
