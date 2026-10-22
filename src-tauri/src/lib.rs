//! furca-desktop: the Tauri door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.
//!
//! The window is the page's; this crate opens repositories, reads them for
//! it off the main thread, and remembers what was open (`state`).

pub mod commands;
pub mod maintenance;
pub mod measure;
pub mod state;

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{Manager, WebviewWindowBuilder};

/// Where the window keeps what it remembers, when the default is not wanted:
/// a measurement, a test, a live run on a copy. The webview's own profile
/// goes inside it too, so nothing of the person's is read or written.
const DATA: &str = "FURCA_DESKTOP_DATA";

/// Build and run the desktop application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let chosen = std::env::var_os(DATA).map(PathBuf::from);
            let dir = match &chosen {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            app.manage(commands::Store {
                state: Mutex::new(state::load(&dir)),
                dir: dir.clone(),
            });

            // The window is described in tauri.conf.json and made here, so a
            // chosen data directory can hold the webview's profile as well.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .ok_or("tauri.conf.json describes no `main` window")?;
            let mut window = WebviewWindowBuilder::from_config(app.handle(), &config)?;
            if chosen.is_some() {
                window = window.data_directory(dir.join("webview"));
            }
            window.build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::window_state,
            commands::open_tab,
            commands::close_tab,
            commands::activate_tab,
            commands::move_tab,
            commands::save_session,
            commands::open_session,
            commands::delete_session,
            commands::line,
            commands::open_line,
            commands::sidebar,
            commands::standing,
            commands::maintain,
            measure::measured,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the furca application");
}
