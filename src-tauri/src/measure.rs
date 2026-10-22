//! What the speed budgets time the window by (`benches/window.rs`).
//!
//! The window cannot be timed from inside: a process does not see how long it
//! took to be started. So the benchmark starts it and waits, and the window
//! says when it has reached a stage - its first frame, every tab read - by
//! writing the stage's name into the file `FURCA_DESKTOP_MEASURE` names.
//! Outside a measurement the variable is unset and every call does nothing.

use std::path::PathBuf;

/// The file stages are written to, when this run is being measured.
const FILE: &str = "FURCA_DESKTOP_MEASURE";
/// The stage after which the window closes itself, when the measurement
/// needs nothing after it.
const EXIT: &str = "FURCA_DESKTOP_MEASURE_EXIT";

/// Records that the window reached `stage`, and closes it when the
/// measurement asked to stop there.
#[tauri::command]
pub async fn measured(app: tauri::AppHandle, stage: String) -> Result<(), String> {
    let Some(file) = std::env::var_os(FILE).map(PathBuf::from) else {
        return Ok(());
    };
    // Written beside and renamed, so the benchmark never reads half a line.
    let mut text = std::fs::read_to_string(&file).unwrap_or_default();
    text.push_str(&stage);
    text.push('\n');
    let next = file.with_extension("next");
    std::fs::write(&next, text).map_err(|e| e.to_string())?;
    std::fs::rename(&next, &file).map_err(|e| e.to_string())?;
    if std::env::var(EXIT).is_ok_and(|exit| exit == stage) {
        app.exit(0);
    }
    Ok(())
}
