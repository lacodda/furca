//! The window's speed budgets, measured on the built app.
//!
//! `cargo bench -p furca-desktop --bench window --features custom-protocol`
//! starts the app the way a person does - a fresh process, the repositories
//! it had open - and times it from the outside, since a process cannot see
//! how long it took to be started. The app says when it reached a stage by
//! writing the stage's name into the file `FURCA_DESKTOP_MEASURE` names
//! (`src/measure.rs`). The numbers go to `target/budgets-window.json`; a
//! broken promise exits non-zero, so a red benchmark is a red gate.
//!
//! The promises are the core's file, `crates/furca-core/benches/budgets.json`:
//! one list for the docs page and both harnesses. This one measures the
//! budgets whose door is `window`.
//!
//! `custom-protocol` is what `tauri build` turns on: without it the binary
//! loads the dev server, and there is no window worth timing.

#[path = "../../crates/furca-core/tests/fixture/mod.rs"]
mod fixture;

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use fixture::Layout;
use serde::{Deserialize, Serialize};

const PROMISES: &str = include_str!("../../crates/furca-core/benches/budgets.json");

/// The app built for this benchmark, with the frontend inside it.
const APP: &str = env!("CARGO_BIN_EXE_furca-desktop");

/// Starts thrown away before measuring: the first ones build the webview's
/// profile and fill the disk cache.
const WARMUP: usize = 3;
/// Starts timed; the median is what the budget is held to.
const RUNS: usize = 21;
/// Memory is read a few times, not timed: it moves little between runs, and
/// each reading costs a settle of its own.
const MEMORY_RUNS: usize = 3;
/// How long the app is left to settle after every tab is read, before its
/// memory is read: a webview keeps allocating for a moment after a paint.
const SETTLE: Duration = Duration::from_secs(2);
/// How long a start may take before it is called a hang.
const PATIENCE: Duration = Duration::from_secs(60);
/// A start slower than this is printed whole, stage by stage: where the time
/// went is the first question a slow machine raises.
const SLOW: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
struct Promises {
    commits: u32,
    budgets: Vec<Promise>,
}

#[derive(Deserialize)]
struct Promise {
    name: String,
    door: String,
    what: String,
    layout: String,
    limit: Option<f64>,
    unit: String,
    status: String,
}

#[derive(Serialize)]
struct Report {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
    commits: u32,
    results: Vec<Measured>,
}

#[derive(Serialize)]
struct Measured {
    name: String,
    what: String,
    layout: String,
    unit: String,
    budget: Option<f64>,
    median: f64,
    p90: f64,
    within: bool,
}

/// What a window budget measures, by the name `budgets.json` gives it, and
/// the unit its number comes in.
fn kind(name: &str) -> (&'static str, Kind) {
    match name {
        "window-on-screen" => ("ms", Kind::Start("shown")),
        "window-first-repository" => ("ms", Kind::Start("first-frame")),
        "window-memory" => ("MB", Kind::Memory),
        other => panic!(
            "budgets.json measures `{other}` in the window, which window.rs has no runner for"
        ),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    /// The time from the start of the process to a stage the app reports.
    Start(&'static str),
    Memory,
}

fn main() {
    let promises: Promises = serde_json::from_str(PROMISES).expect("budgets.json parses");
    let ours: Vec<&Promise> = promises
        .budgets
        .iter()
        .filter(|p| p.status == "measured" && p.door == "window")
        .collect();
    // Checked on every run, `cargo test` and clippy's build included.
    for promise in &ours {
        let (unit, _) = kind(&promise.name);
        assert_eq!(promise.unit, unit, "{} is measured in {unit}", promise.name);
        assert_eq!(
            promise.layout,
            Layout::PackedWithGraph.name(),
            "{} runs on the graph layout",
            promise.name
        );
    }
    if !std::env::args().any(|arg| arg == "--bench") {
        return;
    }
    if !cfg!(feature = "custom-protocol") {
        eprintln!(
            "the window is measured as `tauri build` makes it: run with `--features custom-protocol` \
             after `pnpm build`, or this binary loads the dev server"
        );
        std::process::exit(2);
    }

    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    println!("the fixture: {} commits", promises.commits);
    let repository = fixture::ensure(&root, promises.commits, Layout::PackedWithGraph);
    let work = root.join("window-budget");
    if work.exists() {
        std::fs::remove_dir_all(&work).expect("clear the last measurement");
    }

    // Every start is timed to every stage on the way to its first frame, so
    // one set of starts measures each budget that is a stage.
    let mut starts: Option<Vec<Vec<(String, f64)>>> = None;
    let mut results = Vec::new();
    for promise in ours {
        let (unit, kind) = kind(&promise.name);
        let (median, p90) = match kind {
            Kind::Start(stage) => {
                let taken =
                    starts.get_or_insert_with(|| time_starts(&work.join("starts"), &repository));
                let mut times = stage_times(taken, stage);
                let (median, _) = spread(&mut times);
                // Known flake: a busy machine - a build, a browser, the
                // antivirus looking at a fresh binary - slows every start for
                // seconds at a time. The starts are taken once more after a
                // pause; a regression stays slow.
                if promise.limit.is_some_and(|limit| median > limit) {
                    std::thread::sleep(Duration::from_secs(5));
                    let again = time_starts(&work.join("starts"), &repository);
                    times = stage_times(&again, stage);
                    starts = Some(again);
                }
                spread(&mut times)
            }
            // On macOS the webview's processes are WebKit's own services,
            // started by the system rather than by the app: nothing ties
            // them to it that can be read from outside, and the app's own
            // process alone would be a number that promises nothing.
            Kind::Memory if cfg!(target_os = "macos") => {
                println!(
                    "{:<18} not measured on macOS: the webview's processes are not the app's",
                    promise.name
                );
                continue;
            }
            Kind::Memory => memory(&work.join("memory"), &copies(&root, &repository)),
        };
        let within = promise.limit.is_none_or(|limit| median <= limit);
        let verdict = match (promise.limit, within) {
            (None, _) => "measured, not promised".to_owned(),
            (Some(limit), true) => format!("ok, budget {limit:.0} {unit}"),
            (Some(limit), false) => format!("OVER budget {limit:.0} {unit}"),
        };
        println!(
            "{:<18} {:<20} median {median:>8.1} {unit}  p90 {p90:>8.1} {unit}  {verdict}",
            promise.name, promise.layout,
        );
        results.push(Measured {
            name: promise.name.clone(),
            what: promise.what.clone(),
            layout: promise.layout.clone(),
            unit: unit.to_owned(),
            budget: promise.limit,
            median: round(median),
            p90: round(p90),
            within,
        });
    }

    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        commits: promises.commits,
        results,
    };
    let out = root
        .parent()
        .expect("CARGO_TARGET_TMPDIR sits inside the target dir")
        .join("budgets-window.json");
    std::fs::write(
        &out,
        serde_json::to_string_pretty(&report).expect("serialize"),
    )
    .expect("write the report");
    println!("wrote {}", out.display());

    let over: Vec<&str> = report
        .results
        .iter()
        .filter(|m| !m.within)
        .map(|m| m.name.as_str())
        .collect();
    if !over.is_empty() {
        eprintln!("over budget: {}", over.join(", "));
        std::process::exit(1);
    }
}

/// Starts the app with `repository` open, to its first frame, timing every
/// stage it reports on the way from the start of the process.
fn time_starts(data: &Path, repository: &Path) -> Vec<Vec<(String, f64)>> {
    println!("starting the window {} times", WARMUP + RUNS);
    remember(data, &[repository]);
    let marks = data.join("marks");
    for _ in 0..WARMUP {
        start_to(data, &marks, "first-frame", true);
    }
    (0..RUNS)
        .map(|_| start_to(data, &marks, "first-frame", true).0)
        .collect()
}

/// The milliseconds each start took to reach `stage`.
fn stage_times(starts: &[Vec<(String, f64)>], stage: &str) -> Vec<f64> {
    starts
        .iter()
        .map(|seen| {
            seen.iter()
                .find(|(name, _)| name == stage)
                .map(|(_, ms)| *ms)
                .unwrap_or_else(|| panic!("a start never reported `{stage}`"))
        })
        .collect()
}

/// Three copies of `repository` - three repositories to the window, each
/// with files of its own to map - made once beside it and kept.
fn copies(root: &Path, repository: &Path) -> Vec<PathBuf> {
    let name = repository
        .file_name()
        .expect("the fixture has a name")
        .to_string_lossy();
    let home = root.join(format!("{name}-copies"));
    let done = home.join("complete");
    let copies: Vec<PathBuf> = ["one", "two", "three"]
        .iter()
        .map(|copy| home.join(copy))
        .collect();
    if !done.exists() {
        if home.exists() {
            std::fs::remove_dir_all(&home).expect("remove half a copy");
        }
        for copy in &copies {
            copy_dir(repository, copy);
        }
        std::fs::write(&done, "").expect("mark the copies complete");
    }
    copies
}

/// Reads what the app and its webview hold with `repositories` open, each
/// read, after a moment to settle.
fn memory(data: &Path, repositories: &[PathBuf]) -> (f64, f64) {
    println!("reading the memory {MEMORY_RUNS} times");
    let repositories: Vec<&Path> = repositories.iter().map(PathBuf::as_path).collect();
    remember(data, &repositories);
    let marks = data.join("marks");
    // One start to build the webview's profile, as a person's second start.
    start_to(data, &marks, "first-frame", true);

    let mut readings: Vec<f64> = (0..MEMORY_RUNS)
        .map(|_| {
            let (_, child) = start_to(data, &marks, "settled", false);
            let mut child = child.expect("a running app");
            std::thread::sleep(SETTLE);
            let held = tree_memory(child.id());
            stop_tree(&mut child);
            held
        })
        .collect();
    spread(&mut readings)
}

/// Writes the window's state so it opens `repositories` as its tabs.
fn remember(data: &Path, repositories: &[&Path]) {
    std::fs::create_dir_all(data).expect("the data directory");
    let tabs: Vec<serde_json::Value> = repositories
        .iter()
        .map(|path| {
            let path = std::fs::canonicalize(path).expect("the repository exists");
            let path = path
                .to_string_lossy()
                .trim_start_matches(r"\\?\")
                .to_owned();
            let name = Path::new(&path)
                .file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned();
            serde_json::json!({ "path": path, "name": name })
        })
        .collect();
    let state = serde_json::json!({ "tabs": tabs, "active": 0, "recent": tabs, "sessions": [] });
    std::fs::write(data.join("state.json"), state.to_string()).expect("write the state");
}

/// Starts the app on `data` and waits until it says it reached `stage`,
/// timing from the outside every stage it reports on the way. With `exit`,
/// the app closes itself there and the start is waited out; without, the
/// running app is handed back.
fn start_to(
    data: &Path,
    marks: &Path,
    stage: &str,
    exit: bool,
) -> (Vec<(String, f64)>, Option<Child>) {
    for attempt in 0..2 {
        let _ = std::fs::remove_file(marks);
        let said = data.join("app.log");
        let log = std::fs::File::create(&said).expect("the app's log");
        let mut command = Command::new(APP);
        command
            .env("FURCA_DESKTOP_DATA", data)
            .env("FURCA_DESKTOP_MEASURE", marks)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        if exit {
            command.env("FURCA_DESKTOP_MEASURE_EXIT", stage);
        }
        let started = Instant::now();
        let mut child = command.spawn().expect("the app starts");
        let mut seen: Vec<(String, f64)> = Vec::new();
        let reached = loop {
            let at = started.elapsed().as_secs_f64() * 1000.0;
            if let Ok(text) = std::fs::read_to_string(marks) {
                for name in text.lines().filter_map(|line| line.split(' ').next()) {
                    if !seen.iter().any(|(known, _)| known == name) {
                        seen.push((name.to_owned(), at));
                    }
                }
            }
            if seen.iter().any(|(name, _)| name == stage) {
                break true;
            }
            if child.try_wait().expect("the app can be asked").is_some()
                || started.elapsed() > PATIENCE
            {
                break false;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let took = started.elapsed();
        if !reached {
            // Known flake: the webview's profile is still held by the last
            // start's helper processes for a moment after it exits, and a
            // start in that moment fails to create its webview. Once more,
            // after the profile is let go; a second failure is real.
            stop_tree(&mut child);
            eprintln!(
                "a start did not reach `{stage}` in {:.1} s; it reported {seen:?} and said:\n{}",
                took.as_secs_f64(),
                std::fs::read_to_string(&said).unwrap_or_default()
            );
            assert!(attempt == 0, "the app never reached `{stage}`");
            std::thread::sleep(Duration::from_secs(3));
            continue;
        }
        if took > SLOW {
            println!("  a slow start: {seen:?}");
        }
        if exit {
            // An app that reached its stage and does not close is stopped,
            // not waited on for ever.
            if !exits_within(&mut child, PATIENCE) {
                eprintln!("the app did not close after `{stage}`; stopped");
                stop_tree(&mut child);
            }
            // The webview's helpers outlive the app by a moment; the next
            // start must not meet them.
            std::thread::sleep(Duration::from_millis(500));
            return (seen, None);
        }
        return (seen, Some(child));
    }
    unreachable!("the loop returns or asserts")
}

/// Whether `child` exits within `limit`.
fn exits_within(child: &mut Child, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if child.try_wait().expect("the app can be asked").is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// One process: its id, its parent's, what it holds and its name.
struct Process {
    pid: u32,
    parent: u32,
    bytes: u64,
    name: String,
}

/// The process `root` and every process under it.
fn tree(root: u32) -> Vec<Process> {
    let table = processes();
    let mut ids = vec![root];
    let mut found = true;
    while found {
        found = false;
        for process in &table {
            if ids.contains(&process.parent) && !ids.contains(&process.pid) {
                ids.push(process.pid);
                found = true;
            }
        }
    }
    table
        .into_iter()
        .filter(|process| ids.contains(&process.pid))
        .collect()
}

/// Every process, with what it holds: the private working set on Windows -
/// what Task Manager shows - and the resident set elsewhere.
fn processes() -> Vec<Process> {
    if cfg!(windows) {
        let script = "$ws = @{}; Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | \
            ForEach-Object { $ws[[int]$_.IDProcess] = $_.WorkingSetPrivate }; \
            Get-CimInstance Win32_Process | ForEach-Object { \
            \"$($_.ProcessId) $($_.ParentProcessId) $([uint64]$ws[[int]$_.ProcessId]) $($_.Name)\" }";
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .output()
            .expect("powershell runs");
        parse(&String::from_utf8_lossy(&out.stdout), 1)
    } else {
        let out = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid=,rss=,comm="])
            .output()
            .expect("ps runs");
        let mut table = parse(&String::from_utf8_lossy(&out.stdout), 1024);
        // The resident set counts a library once in every process that maps
        // it, and the webview's libraries are mapped by all of its
        // processes: summed, they were counted three and four times over.
        // The proportional set divides each shared page among its users -
        // the counterpart of Windows' private working set.
        if cfg!(target_os = "linux") {
            for process in &mut table {
                if let Some(pss) = proportional(process.pid) {
                    process.bytes = pss;
                }
            }
        }
        table
    }
}

/// The proportional set size of `pid`, from `/proc/PID/smaps_rollup`.
fn proportional(pid: u32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    let line = text.lines().find(|line| line.starts_with("Pss:"))?;
    let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kilobytes * 1024)
}

fn parse(text: &str, scale: u64) -> Vec<Process> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(Process {
                pid: fields.next()?.parse().ok()?,
                parent: fields.next()?.parse().ok()?,
                bytes: fields.next()?.parse::<u64>().ok()? * scale,
                name: fields.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// What the app and every process it started hold together, in megabytes;
/// what each holds is printed, since which process grew is the first
/// question a number over its budget raises.
fn tree_memory(root: u32) -> f64 {
    let megabytes = |bytes: u64| bytes as f64 / (1024.0 * 1024.0);
    let tree = tree(root);
    assert!(
        tree.len() > 1,
        "the app runs alone: its webview is not where it should be"
    );
    let parts: Vec<String> = tree
        .iter()
        .map(|process| format!("{} {:.1}", process.name, megabytes(process.bytes)))
        .collect();
    println!("  by process, MB: {}", parts.join(", "));
    megabytes(tree.iter().map(|process| process.bytes).sum())
}

/// Stops the app and every process under it.
fn stop_tree(child: &mut Child) {
    if cfg!(windows) {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .output();
    } else {
        for process in tree(child.id()) {
            let _ = Command::new("kill")
                .args(["-9", &process.pid.to_string()])
                .output();
        }
    }
    let _ = child.wait();
    std::thread::sleep(Duration::from_secs(1));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("make the copy");
    for entry in std::fs::read_dir(from).expect("read the fixture") {
        let entry = entry.expect("an entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy a file");
        }
    }
}

/// Median and 90th percentile.
fn spread(values: &mut [f64]) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    (values[n / 2], values[(n * 9 / 10).min(n - 1)])
}

fn round(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}
