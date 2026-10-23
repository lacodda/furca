//! What the speed budgets time the window by (`benches/window.rs`).
//!
//! The window cannot be timed from inside: a process does not see how long it
//! took to be started. So the benchmark starts it and waits, and the window
//! says when it has reached a stage - its first frame, every tab read - by
//! writing the stage's name into the file `FURCA_DESKTOP_MEASURE` names, with
//! the milliseconds since `run` began beside it, so a slow start says where it
//! was slow. Outside a measurement the variable is unset and nothing is
//! written.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// The file stages are written to, when this run is being measured.
const FILE: &str = "FURCA_DESKTOP_MEASURE";
/// The stage after which the window closes itself, when the measurement
/// needs nothing after it.
const EXIT: &str = "FURCA_DESKTOP_MEASURE_EXIT";

/// When `run` began: the first moment the app's own code runs.
static STARTED: OnceLock<Instant> = OnceLock::new();
/// Held while the file is rewritten: stages arrive from the main thread and
/// from commands on other threads at once, and two rewrites at a time would
/// each keep only their own line.
static WRITING: Mutex<()> = Mutex::new(());

/// Marks the start of the app's own code; the stages are counted from here.
pub fn start() {
    STARTED.get_or_init(Instant::now);
}

/// Writes `stage` with its time into the measurement's file, if this run is
/// measured. Returns whether it was.
pub fn mark(stage: &str) -> bool {
    let Some(file) = std::env::var_os(FILE).map(PathBuf::from) else {
        return false;
    };
    let millis = STARTED
        .get()
        .map_or(0, |started| started.elapsed().as_millis());
    let _writing = WRITING.lock();
    // Written beside and renamed, so the benchmark never reads half a line.
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&format!("{stage} {millis}\n"));
    let next = file.with_extension("next");
    std::fs::write(&next, text)
        .and_then(|()| std::fs::rename(&next, &file))
        .is_ok()
}

/// Records that the window reached `stage`, and closes it when the
/// measurement asked to stop there.
#[tauri::command]
pub async fn measured(app: tauri::AppHandle, stage: String) -> Result<(), String> {
    if mark(&stage) && std::env::var(EXIT).is_ok_and(|exit| exit == stage) {
        app.exit(0);
    }
    Ok(())
}
