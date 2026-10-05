//! Versions written into prose: samples of output, install examples, pinned
//! downloads. A manifest's version is checked by reading the manifest; these
//! are the ones nothing else reads, and they go stale the moment the next
//! release ships.
//!
//! A mention is this product's version when it is written `v1.2.3`, or
//! follows one of the product's names (`furca 1.2.3`, `furca@1.2.3`). A
//! bare `1.2.3` is left alone: in prose it is as often a dependency's number
//! as ours, and an example that means "some version" writes the shape,
//! `vX.Y.Z`, which is no version at all.
//!
//! Some numbers are history - the release that first did a thing - and
//! rewriting them would delete the fact they carry. The line marks those
//! blocks the way kasl's docs began to: a line `<!-- historical versions -->`
//! opens one and `<!-- /historical versions -->` closes it (`#` or `//`
//! comments in files that are not markup). The end is stated, never guessed
//! from the shape of the text, so an exemption cannot quietly spread to the
//! prose after it; a block left open is reported.

use serde::Serialize;

/// One version written into a text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mention {
    /// The file, relative to the root.
    pub path: String,
    /// 1-based.
    pub line: usize,
    /// What is written, such as `v0.2.0` or `furca 0.2.0`.
    pub text: String,
    /// The version alone.
    pub version: String,
    /// Inside a historical-versions block.
    pub history: bool,
}

/// What one text holds: its mentions, and a historical block it opened and
/// never closed.
#[derive(Debug, Default)]
pub struct Scan {
    pub mentions: Vec<Mention>,
    /// The 1-based line of a block still open at the end of the text.
    pub unclosed: Option<usize>,
}

/// Files whose versions are someone else's or are generated: lockfiles pin
/// dependencies, never the product.
const LOCKFILES: [&str; 6] = [
    "Cargo.lock",
    "pnpm-lock.yaml",
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "bun.lock",
];

/// The kinds of text that carry samples and examples: documentation,
/// workflows, installers, manifests and configuration. Source code is left
/// out - its version strings are test data and parsers, not claims.
const EXTENSIONS: [&str; 18] = [
    "md", "mdx", "markdown", "txt", "yml", "yaml", "json", "toml", "ps1", "psm1", "sh", "bash",
    "zsh", "html", "astro", "js", "mjs", "cjs",
];

/// Whether the scanner reads the file at `path` at all.
pub fn scanned(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    if LOCKFILES.contains(&name) {
        return false;
    }
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()))
}

/// Every mention of a version of the product in `text`.
pub fn scan(path: &str, text: &str, names: &[String]) -> Scan {
    let mut out = Scan::default();
    for (index, line) in text.lines().enumerate() {
        match marker(line) {
            Some(Marker::Open) => {
                out.unclosed.get_or_insert(index + 1);
                continue;
            }
            Some(Marker::Close) => {
                out.unclosed = None;
                continue;
            }
            None => {}
        }
        for (start, end, version) in mentions_in(line, names) {
            out.mentions.push(Mention {
                path: path.to_owned(),
                line: index + 1,
                text: line[start..end].to_owned(),
                version,
                history: out.unclosed.is_some(),
            });
        }
    }
    out
}

enum Marker {
    Open,
    Close,
}

/// A line that opens or closes a historical-versions block: the words alone
/// in a comment of any of the kinds the scanned files use.
fn marker(line: &str) -> Option<Marker> {
    let mut text = line.trim();
    for (open, close) in [("<!--", "-->"), ("{/*", "*/}"), ("/*", "*/")] {
        if let Some(inner) = text.strip_prefix(open).and_then(|t| t.strip_suffix(close)) {
            text = inner.trim();
            break;
        }
    }
    for prefix in ["#", "//"] {
        if let Some(inner) = text.strip_prefix(prefix) {
            text = inner.trim();
            break;
        }
    }
    match text {
        "historical versions" => Some(Marker::Open),
        "/historical versions" => Some(Marker::Close),
        _ => None,
    }
}

/// The spans of version mentions in one line, as `(start, end, version)`.
fn mentions_in(line: &str, names: &[String]) -> Vec<(usize, usize, String)> {
    let bytes = line.as_bytes();
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let rest = &line[at..];
        let mut hit = None;

        for name in names {
            if !rest.starts_with(name.as_str()) || !name_boundary_before(bytes, at) {
                continue;
            }
            let after = at + name.len();
            let Some(&joint) = bytes.get(after) else {
                continue;
            };
            if joint != b' ' && joint != b'@' {
                continue;
            }
            let mut number = after + 1;
            if bytes.get(number) == Some(&b'v') {
                number += 1;
            }
            if let Some(end) = version_at(line, number) {
                hit = Some((at, end, line[number..end].to_owned()));
                break;
            }
        }

        if hit.is_none()
            && bytes[at] == b'v'
            && v_boundary_before(bytes, at)
            && let Some(end) = version_at(line, at + 1)
        {
            hit = Some((at, end, line[at + 1..end].to_owned()));
        }

        match hit {
            Some((start, end, version)) => {
                found.push((start, end, version));
                at = end;
            }
            None => at += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// A `v` starts a version unless it ends a word or a reference such as
/// `@v4`; `furca-v0.2.0` in an archive name is a mention.
fn v_boundary_before(bytes: &[u8], at: usize) -> bool {
    at == 0 || {
        let before = bytes[at - 1];
        !(before.is_ascii_alphanumeric() || matches!(before, b'_' | b'.' | b'@'))
    }
}

/// A name starts a mention only as a whole word: `xfurca 1.0.0` is not one.
fn name_boundary_before(bytes: &[u8], at: usize) -> bool {
    at == 0 || {
        let before = bytes[at - 1];
        !(before.is_ascii_alphanumeric() || matches!(before, b'_' | b'-' | b'.'))
    }
}

/// The end of a semantic version starting at `at`, if one does: three
/// numbers, an optional pre-release, and nothing glued on after.
fn version_at(line: &str, at: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut end = at;
    for part in 0..3 {
        let digits = bytes[end..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        end += digits;
        if part < 2 {
            if bytes.get(end) != Some(&b'.') {
                return None;
            }
            end += 1;
        }
    }
    let core = end;
    let mut ends = Vec::with_capacity(2);
    if bytes.get(core) == Some(&b'-') {
        let tail = bytes[core + 1..]
            .iter()
            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'.' || **b == b'-')
            .count();
        let mut stop = core + 1 + tail;
        while stop > core + 1 && matches!(bytes[stop - 1], b'.' | b'-') {
            stop -= 1;
        }
        if stop > core + 1 {
            ends.push(stop);
        }
    }
    // Without the pre-release: `furca-v0.2.0-x86_64` is 0.2.0 followed by a
    // target, not the pre-release `x86`.
    ends.push(core);
    ends.into_iter().find(|&end| {
        let glued = match bytes.get(end) {
            Some(b) if b.is_ascii_alphanumeric() || *b == b'_' => true,
            Some(b'.') => bytes.get(end + 1).is_some_and(u8::is_ascii_digit),
            _ => false,
        };
        !glued && semver::Version::parse(&line[at..end]).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(line: &str) -> Vec<String> {
        let names = vec!["furca".to_owned(), "furca-core".to_owned()];
        scan("x.md", line, &names)
            .mentions
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    #[test]
    fn a_v_number_and_a_named_number_are_mentions() {
        assert_eq!(versions("Install v0.2.0 today"), ["v0.2.0"]);
        assert_eq!(versions("$ furca --version\nfurca 0.2.0"), ["furca 0.2.0"]);
        assert_eq!(versions("cargo install furca@0.2.0"), ["furca@0.2.0"]);
        assert_eq!(versions("furca v0.2.0 is out"), ["furca v0.2.0"]);
        assert_eq!(
            versions("releases/download/v0.2.0/furca-v0.2.0-x86_64.zip"),
            ["v0.2.0", "v0.2.0"]
        );
        assert_eq!(versions("e.g. FURCA_VERSION=v1.0.0-rc.1."), ["v1.0.0-rc.1"]);
    }

    #[test]
    fn other_numbers_are_left_alone() {
        assert!(versions("gix 0.88.0 and Tauri 2.12.1").is_empty());
        assert!(versions("uses: actions/checkout@v7.0.1").is_empty());
        assert!(versions("version 1.2.3.4 and v1.2").is_empty());
        assert!(versions("dev1.2.3 nav0.1.0 v1.2.3x").is_empty());
        assert!(versions("xfurca 1.0.0 and furca-desktop 1.0.0").is_empty());
        assert!(versions("pnpm@10.34.5").is_empty());
    }

    #[test]
    fn a_historical_block_holds_history_up_to_its_stated_end() {
        let names = vec!["furca".to_owned()];
        let text = "Install vX.Y.Z or v0.3.0.\n\
                    <!-- historical versions -->\n\
                    | v0.1.0 | status |\n\
                    Since v0.2.0 the log is ordered.\n\
                    <!-- /historical versions -->\n\
                    Now v0.2.0.\n\
                    # historical versions\n\
                    # v0.0.1 was never published\n\
                    # /historical versions\n";
        let found: Vec<(usize, String, bool)> = scan("a.md", text, &names)
            .mentions
            .into_iter()
            .map(|m| (m.line, m.text, m.history))
            .collect();
        assert_eq!(
            found,
            [
                (1, "v0.3.0".to_owned(), false),
                (3, "v0.1.0".to_owned(), true),
                (4, "v0.2.0".to_owned(), true),
                (6, "v0.2.0".to_owned(), false),
                (8, "v0.0.1".to_owned(), true),
            ]
        );
    }

    #[test]
    fn a_block_left_open_is_reported_where_it_opened() {
        let names = vec!["furca".to_owned()];
        let scan = scan("a.md", "a\n<!-- historical versions -->\nv0.1.0\n", &names);
        assert_eq!(scan.unclosed, Some(2));
        assert!(scan.mentions[0].history);
        assert_eq!(a_closed_block(), None);
    }

    fn a_closed_block() -> Option<usize> {
        let names = vec!["furca".to_owned()];
        scan(
            "a.md",
            "<!-- historical versions -->\nv0.1.0\n<!-- /historical versions -->\n",
            &names,
        )
        .unclosed
    }

    #[test]
    fn lockfiles_and_source_code_are_not_read() {
        assert!(scanned("README.md"));
        assert!(scanned(".github/workflows/publish.yml"));
        assert!(scanned("tools/install.ps1"));
        assert!(scanned("npm/package.json"));
        assert!(!scanned("Cargo.lock"));
        assert!(!scanned("docs/pnpm-lock.yaml"));
        assert!(!scanned("src/main.rs"));
        assert!(!scanned("LICENSE"));
    }
}
