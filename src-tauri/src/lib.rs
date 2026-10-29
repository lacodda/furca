//! furca-desktop: the Tauri door onto furca-core. See
//! `docs/adr/0002-one-core-three-doors.md`.
//!
//! The window is the page's; this crate opens repositories, reads them for
//! it off the main thread, and remembers what was open (`state`).

pub mod commands;
pub mod details;
pub mod graph;
pub mod maintenance;
pub mod measure;
mod nooma;
pub mod state;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::webview::{Color, WebviewBuilder};
use tauri::window::WindowBuilder;
use tauri::{Manager, PhysicalPosition, Theme};

/// Where the window keeps what it remembers, when the default is not wanted:
/// a measurement, a test, a live run on a copy. The webview's own profile
/// goes inside it too, so nothing of the person's is read or written.
const DATA: &str = "FURCA_DESKTOP_DATA";

/// The window's ground before the page has drawn anything, in the system's
/// theme: dowel's ground with furca's 6% tint, as the theme computes it and
/// as `index.html` writes it out. A window that came up in the other colour
/// would flash when the page arrives.
const DARK_GROUND: Color = Color(0x1d, 0x18, 0x1a, 0xff);
const LIGHT_GROUND: Color = Color(0xf5, 0xed, 0xed, 0xff);

/// The repositories a start of furca was asked to open: every argument that
/// is not a flag, as a path, relative to where it was started from.
fn paths(args: impl IntoIterator<Item = String>, cwd: &Path) -> Vec<PathBuf> {
    args.into_iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .map(|arg| cwd.join(arg))
        .collect()
}

/// Build and run the desktop application.
pub fn run() {
    measure::start();
    let chosen = std::env::var_os(DATA).map(PathBuf::from);
    let mut builder = tauri::Builder::default();
    // One window per data directory: two windows on one store would each
    // save its own tabs over the other's. A second start hands its paths to
    // the window already open and brings that window forward. A run on a
    // chosen directory - a measurement, a test - is a store of its own and
    // neither absorbs the person's window nor is absorbed by it; the plugin
    // knows the app only by its identifier, so it stays out of such a run.
    if chosen.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let paths = paths(args, Path::new(&cwd));
            if !paths.is_empty() {
                let app = app.clone();
                std::thread::spawn(move || {
                    app.state::<commands::Store>()
                        .open_from_outside(&app, &paths);
                });
            }
        }));
    }
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            measure::mark("setup");
            let dir = match &chosen {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            let store = commands::Store {
                state: Mutex::new(state::load(&dir)),
                dir: dir.clone(),
            };
            // `furca-desktop PATH` opens PATH, in front of what was open.
            let asked = paths(std::env::args(), &std::env::current_dir()?);
            if !asked.is_empty()
                && let Ok(opened) = store.open_paths(app.handle(), &asked)
            {
                for failed in opened.failed {
                    eprintln!("furca: {}: {}", failed.path, failed.reason);
                }
            }
            let graphs = graph::Graphs::default();
            let early = match store.state.lock() {
                Ok(state) => {
                    graphs.begin_early(&state);
                    commands::Early::start(&state)
                }
                Err(_) => commands::Early::default(),
            };
            app.manage(store);
            app.manage(early);
            app.manage(graphs);

            // The window is on the screen before its webview is (ADR 0011):
            // the webview takes some 350 ms to start, and a window that waits
            // for it is a click that seems to do nothing. So the window is
            // made first, in the ground of the system's theme, shown, and the
            // webview is put in it after. The window is described in
            // tauri.conf.json and made here, so a chosen data directory can
            // hold the webview's profile as well.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .cloned()
                .ok_or("tauri.conf.json describes no `main` window")?;
            let window = WindowBuilder::from_config(app.handle(), &config)?.build()?;
            let ground = match window.theme() {
                Ok(Theme::Light) => LIGHT_GROUND,
                _ => DARK_GROUND,
            };
            window.set_background_color(Some(ground))?;
            window.show()?;
            measure::mark("shown");

            let mut webview = WebviewBuilder::from_config(&config)
                .auto_resize()
                .background_color(ground);
            if chosen.is_some() {
                webview = webview.data_directory(dir.join("webview"));
            }
            window.add_child(webview, PhysicalPosition::new(0, 0), window.inner_size()?)?;
            measure::mark("window");
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
            commands::set_layout,
            commands::line,
            commands::open_line,
            commands::sidebar,
            commands::standing,
            commands::maintain,
            graph::graph,
            graph::graph_rows,
            graph::graph_position,
            graph::graph_find,
            graph::forget_graph,
            details::details,
            details::file_diff,
            measure::measured,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the furca application");
}

#[cfg(test)]
mod tests {
    use super::paths;
    use std::path::{Path, PathBuf};

    #[test]
    fn a_start_opens_its_arguments_but_not_its_flags_or_itself() {
        let cwd = Path::new("work");
        let args = ["furca-desktop", "--verbose", "repo", "other/one"].map(String::from);
        assert_eq!(
            paths(args, cwd),
            [
                PathBuf::from("work").join("repo"),
                PathBuf::from("work").join("other/one")
            ]
        );
        assert!(paths(["furca-desktop".to_owned()], cwd).is_empty());
    }
}
