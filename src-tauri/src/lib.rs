//! furca-desktop: the Tauri door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.

use furca_core::HeadSummary;

/// Opens the repository at `path` and reports the state of its `HEAD`.
///
/// Errors are returned as plain strings: Tauri serializes a command's `Err`
/// straight to the frontend, and a `Display`-formatted message is all the
/// window needs to show.
#[tauri::command]
fn open_repository(path: String) -> Result<HeadSummary, String> {
    let repo = furca_core::Repository::open(&path).map_err(|e| e.to_string())?;
    repo.head().map_err(|e| e.to_string())
}

/// Build and run the desktop application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![open_repository])
        .run(tauri::generate_context!())
        .expect("error while running the furca application");
}
