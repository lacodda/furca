// A console window would appear behind the app on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    furca_desktop_lib::run()
}
