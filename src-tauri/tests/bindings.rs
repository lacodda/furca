//! The window's types, generated from the Rust ones that cross IPC (ADR 0009).
//!
//! Every argument and return type of every command, and every event payload,
//! is exported with [`ts_rs`] together with everything it depends on into a
//! temporary directory, and compared file by file with the committed
//! `src/lib/generated/`. A mismatch fails with what changed and this:
//!
//!     FURCA_BLESS=1 cargo test -p furca-desktop --test bindings
//!
//! which writes `src/lib/generated/` again - deleting it first, so a type that
//! stopped crossing the wire leaves rather than lingering stale.
//!
//! `#[ts(export)]` is not used on the types: it would make every `cargo test`
//! anywhere write into the tree. This is the one place that exports, and only
//! when asked to.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ts_rs::{Config, TS};

/// Where the window keeps its copy.
fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("src-tauri has a parent")
        .join("src/lib/generated")
}

/// Exports every root type, and what it depends on, into `dir`: what the
/// commands in `src/commands.rs` return and the payload of the maintenance
/// event. Their dependencies are followed by `export_all` itself.
fn export_all(dir: &Path) -> Result<(), ts_rs::ExportError> {
    let cfg = Config::from_env().with_out_dir(dir);

    macro_rules! export {
        ($($ty:ty),* $(,)?) => {
            $(<$ty as TS>::export_all(&cfg)?;)*
        };
    }

    export! {
        furca_desktop_lib::state::WindowState,
        furca_desktop_lib::commands::Opened,
        furca_desktop_lib::commands::Sidebar,
        furca_desktop_lib::commands::Line,
        furca_core::Standing,
        furca_desktop_lib::maintenance::Maintenance,
    }
    Ok(())
}

/// Every file under `dir`, relative to it, with its contents; `\r\n` folded
/// to `\n` so a Windows checkout compares equal to what was just written.
fn read_tree(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    if !dir.exists() {
        return out;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("the directory reads") {
            let path = entry.expect("an entry reads").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path.strip_prefix(dir).expect("under dir").to_path_buf();
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_default()
                    .replace("\r\n", "\n");
                out.insert(relative, text);
            }
        }
    }
    out
}

#[test]
fn the_generated_types_match_the_ones_that_cross_ipc() {
    let committed_dir = generated_dir();
    if std::env::var_os("FURCA_BLESS").is_some() {
        // Deleted first: `export_all` only ever adds files.
        if committed_dir.exists() {
            std::fs::remove_dir_all(&committed_dir).expect("the old folder goes");
        }
        std::fs::create_dir_all(&committed_dir).expect("the folder is made");
        export_all(&committed_dir).expect("the types export");
        return;
    }

    let fresh = tempfile::tempdir().expect("a temporary directory");
    export_all(fresh.path()).expect("the types export");
    let expected = read_tree(fresh.path());
    let committed = read_tree(&committed_dir);

    let expected_paths: BTreeSet<&PathBuf> = expected.keys().collect();
    let committed_paths: BTreeSet<&PathBuf> = committed.keys().collect();
    let missing: Vec<_> = expected_paths.difference(&committed_paths).collect();
    let extra: Vec<_> = committed_paths.difference(&expected_paths).collect();
    let different: Vec<_> = expected_paths
        .intersection(&committed_paths)
        .filter(|path| expected[**path] != committed[**path])
        .collect();

    assert!(
        missing.is_empty() && extra.is_empty() && different.is_empty(),
        "src/lib/generated/ no longer matches the types that cross IPC.\n\
         missing: {missing:?}\nextra: {extra:?}\ndifferent: {different:?}\n\
         Write it again with `FURCA_BLESS=1 cargo test -p furca-desktop --test bindings`."
    );
}

/// The root list above is written by hand, so it is held to the commands: a
/// type a command names that has no generated file is one the window would
/// have to write by hand again.
#[test]
fn every_type_a_command_names_is_generated() {
    if std::env::var_os("FURCA_BLESS").is_some() {
        return;
    }
    // Tauri's and std's names, which are not shapes on the wire.
    const NOT_ON_THE_WIRE: [&str; 6] = ["State", "AppHandle", "Store", "Result", "String", "Vec"];

    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let committed = read_tree(&generated_dir());
    let mut named = BTreeSet::new();
    for file in ["commands.rs", "measure.rs"] {
        let text = std::fs::read_to_string(src.join(file)).expect("the source reads");
        let mut rest = text.as_str();
        while let Some(at) = rest.find("#[tauri::command]") {
            rest = &rest[at..];
            let open = rest
                .find("pub async fn ")
                .expect("a command is a pub async fn");
            let body = rest[open..].find('{').expect("a command has a body") + open;
            let signature = &rest[open..body];
            for word in signature.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if word.starts_with(char::is_uppercase) && !NOT_ON_THE_WIRE.contains(&word) {
                    named.insert(word.to_owned());
                }
            }
            rest = &rest[body..];
        }
    }
    assert!(!named.is_empty(), "the scan found no command types at all");
    let unexported: Vec<_> = named
        .iter()
        .filter(|name| !committed.contains_key(Path::new(&format!("{name}.ts"))))
        .collect();
    assert!(
        unexported.is_empty(),
        "commands name types with no generated file: {unexported:?}; add them to export_all"
    );
}
