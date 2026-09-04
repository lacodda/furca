//! Guards the facts that must agree across every shop window before a release.
//!
//! Versions drift between manifests the moment one of them is bumped by hand,
//! and a second README appears the moment someone needs a shorter one. Both
//! failures are only visible after publishing, so they are checked here.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/furca-cli has a parent")
        .parent()
        .expect("crates/ has a parent")
        .to_path_buf()
}

fn read(relative: &str) -> String {
    let path = repo_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// First `"version": "..."` value in a JSON document.
fn json_version(source: &str) -> String {
    source
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("\"version\"")?;
            let value = rest.trim_start_matches([':', ' ']);
            value.trim_matches(['"', ',']).to_owned().into()
        })
        .expect("no `version` field found")
}

/// First `version = "..."` value in a Cargo.toml.
fn toml_version(source: &str) -> Option<String> {
    source.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("version")?;
        let rest = rest.trim_start();
        let rest = rest.strip_prefix('=')?;
        let value = rest.trim();
        if value.starts_with("{") {
            return None; // `version.workspace = true` — not a literal version.
        }
        Some(value.trim_matches('"').to_owned())
    })
}

#[test]
fn every_manifest_declares_the_same_version() {
    let workspace_toml = read("Cargo.toml");
    let workspace_version = toml_version(&workspace_toml)
        .expect("the workspace Cargo.toml declares [workspace.package].version");

    assert_eq!(
        json_version(&read("package.json")),
        workspace_version,
        "package.json disagrees with the workspace Cargo.toml"
    );
    assert_eq!(
        json_version(&read("src-tauri/tauri.conf.json")),
        workspace_version,
        "tauri.conf.json disagrees with the workspace Cargo.toml — the installer would carry the wrong version"
    );

    // src-tauri/Cargo.toml may declare its own version or inherit the
    // workspace's; either is fine as long as it resolves to the same number.
    let tauri_toml = read("src-tauri/Cargo.toml");
    if let Some(declared) = toml_version(&tauri_toml) {
        assert_eq!(
            declared, workspace_version,
            "src-tauri/Cargo.toml declares its own version, and it disagrees with the workspace"
        );
    } else {
        assert!(
            tauri_toml.contains("version.workspace = true"),
            "src-tauri/Cargo.toml neither declares its own version nor inherits the workspace's"
        );
    }
}

#[test]
fn there_is_exactly_one_readme() {
    let extra = ["docs/README.md", "src-tauri/README.md", "npm/README.md"];

    for candidate in extra {
        assert!(
            !repo_root().join(candidate).exists(),
            "{candidate} is a second README; the root one is the only source"
        );
    }
}

#[test]
fn readme_links_are_absolute() {
    let readme = read("README.md");

    // A relative link works on GitHub and breaks everywhere the README is
    // republished — crates.io, npm, the docs site.
    for line in readme.lines() {
        let Some(start) = line.find("](") else {
            continue;
        };
        let target = &line[start + 2..];
        let target = &target[..target.find(')').unwrap_or(target.len())];

        assert!(
            target.starts_with("http") || target.starts_with('#'),
            "relative link `{target}` in README.md"
        );
    }
}

/// The copies of the mark that must be byte-identical to each other.
///
/// The repo does not check these against the vault master (CI cannot see the
/// author's machine); it checks that the copies committed under `assets/` and
/// `docs/` agree with each other, so a hand-edit or a partial re-export shows
/// up as a failure here instead of as a mismatched mark on one surface.
#[test]
fn the_brand_svgs_are_identical_copies() {
    let root = repo_root();
    let logo = std::fs::read(root.join("assets/logo.svg")).expect("assets/logo.svg is missing");

    let docs_logo = std::fs::read(root.join("docs/src/assets/logo.svg"))
        .expect("docs/src/assets/logo.svg is missing");
    assert_eq!(
        docs_logo, logo,
        "docs/src/assets/logo.svg has drifted from assets/logo.svg — both must be the L mark, byte for byte"
    );

    let s = std::fs::read(root.join("assets/logo-s.svg")).expect("assets/logo-s.svg is missing");
    let favicon = std::fs::read(root.join("docs/public/favicon.svg"))
        .expect("docs/public/favicon.svg is missing");
    assert_eq!(
        favicon, s,
        "docs/public/favicon.svg has drifted from assets/logo-s.svg — both must be the S mark, byte for byte"
    );

    // logo-m.svg and banner.svg have no second copy elsewhere in the repo
    // today; they still have to exist so a future copy has something to
    // match.
    for solo in ["assets/logo-m.svg", "assets/banner.svg"] {
        assert!(root.join(solo).exists(), "{solo} is missing");
    }
}

/// The icon the application ships with is the one in `assets/`.
///
/// Both are written by `docs/export-assets.mjs`; if this fails, run it.
#[test]
fn the_application_icon_matches_the_brand_icon() {
    let root = repo_root();
    let ours = std::fs::read(root.join("src-tauri/icons/icon.ico"))
        .expect("the application icon is missing");
    let brand = std::fs::read(root.join("assets/icon.ico")).expect("the brand icon is missing");

    assert_eq!(
        ours, brand,
        "src-tauri/icons/icon.ico has fallen behind assets/icon.ico — re-run docs/export-assets.mjs"
    );
}

/// The icon's largest image comes first.
///
/// Windows itself does not care — it picks the entry closest to the size it
/// asked for. `tauri-codegen` does: it takes `entries()[0]` verbatim as the
/// window icon, so whatever is first is what the titlebar is scaled from. A
/// 16x16 first entry means the titlebar is stretched from sixteen pixels and
/// looks smeared.
#[test]
fn the_largest_icon_image_comes_first() {
    let ico =
        std::fs::read(repo_root().join("assets/icon.ico")).expect("the brand icon is missing");

    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    assert!(count > 1, "an icon with one size cannot serve every place");

    // Byte 0 of each 16-byte directory entry is the width; 0 encodes 256.
    let widths: Vec<u32> = (0..count)
        .map(|index| match ico[6 + 16 * index] {
            0 => 256,
            width => u32::from(width),
        })
        .collect();

    let mut descending = widths.clone();
    descending.sort_unstable_by(|a, b| b.cmp(a));

    assert_eq!(
        widths, descending,
        "icon.ico must list its images largest first. Re-run docs/export-assets.mjs"
    );
}

/// One entry of an `.ico` directory: just enough to find and decode its
/// payload. Mirrors the layout `docs/export-assets.mjs` writes.
struct IcoEntry {
    size: u32,
    offset: usize,
    length: usize,
}

fn ico_entries(ico: &[u8]) -> Vec<IcoEntry> {
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    (0..count)
        .map(|index| {
            let at = 6 + 16 * index;
            let size = match ico[at] {
                0 => 256,
                width => u32::from(width),
            };
            let length =
                u32::from_le_bytes([ico[at + 8], ico[at + 9], ico[at + 10], ico[at + 11]]) as usize;
            let offset =
                u32::from_le_bytes([ico[at + 12], ico[at + 13], ico[at + 14], ico[at + 15]])
                    as usize;
            IcoEntry {
                size,
                offset,
                length,
            }
        })
        .collect()
}

/// Every size in `assets/icon.ico` carries the level of the mark that reads
/// at it.
///
/// L and M plate the mark in near-black (`#1B2126`) with a gradient outline
/// and code; S fills the whole hexagon with the gradient and sets dark text
/// on it, which is the only rendering that still reads as a colour block at
/// icon sizes. A sample a quarter of the way across the image, vertically
/// centred, lands inside the hexagon and away from the code glyphs, so its
/// brightness tells the two renderings apart: the filled S tile is bright
/// (measured ~230-300 on R+G+B at 16-24px), the near-black plate is dark
/// (~100 at every size M and up).
///
/// Proven by rejection: temporarily pointing a plated size (32px+) at the S
/// export in `docs/export-assets.mjs` and rebuilding `assets/icon.ico` makes
/// this test fail; reverting and rebuilding makes it pass again.
#[test]
fn every_size_carries_the_level_that_reads_at_it() {
    let ico =
        std::fs::read(repo_root().join("assets/icon.ico")).expect("the brand icon is missing");

    for entry in ico_entries(&ico) {
        let payload = &ico[entry.offset..entry.offset + entry.length];
        let decoder = png::Decoder::new(std::io::Cursor::new(payload));
        let mut reader = decoder
            .read_info()
            .unwrap_or_else(|e| panic!("the {}px entry is not a PNG: {e}", entry.size));
        let mut buf = vec![
            0;
            reader
                .output_buffer_size()
                .expect("PNG header declares no size")
        ];
        let info = reader
            .next_frame(&mut buf)
            .unwrap_or_else(|e| panic!("cannot decode the {}px entry: {e}", entry.size));
        let bytes = &buf[..info.buffer_size()];
        let channels = info.color_type.samples();
        assert_eq!(
            info.width, entry.size,
            "the {}px entry holds a {}px image",
            entry.size, info.width
        );

        // A quarter of the way across, vertically centred: inside the
        // hexagon, clear of the code and the graph trace beneath it.
        let x = (info.width / 4) as usize;
        let y = (info.height / 2) as usize;
        let stride = info.width as usize * channels;
        let at = y * stride + x * channels;
        let sample = &bytes[at..at + channels];

        assert!(
            sample.len() >= 3,
            "the {}px image has fewer than 3 colour channels",
            entry.size
        );
        let brightness = u32::from(sample[0]) + u32::from(sample[1]) + u32::from(sample[2]);
        let filled = brightness > 180;

        if entry.size <= 24 {
            assert!(
                filled,
                "the {}px image is not the filled S tile (sample {sample:?}); at 24px and below only the filled tile reads as a colour block",
                entry.size
            );
        } else {
            assert!(
                !filled,
                "the {}px image is the filled S tile (sample {sample:?}), not the plated M/L mark — the level rule puts S at 24px and below",
                entry.size
            );
        }
    }
}
