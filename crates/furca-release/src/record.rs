//! The owner's record of the project: which version the current stage aims
//! at, under what title and for which Friday. The record is rigger's; furca
//! asks it through its CLI and never writes to it here.
//!
//! The record is optional. Without rigger on the machine, or without the
//! project in it, the plan proposes a number from the commits alone and says
//! so.

use std::path::Path;
use std::process::Command;

use semver::Version;
use serde::Serialize;

/// The stage the record says is being built.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Stage {
    pub version: Version,
    /// The stage's title, in the record's own language.
    pub title: String,
    /// The calendar week it is aimed at, such as `2026-W41`.
    pub week: Option<String>,
    /// The Friday it should ship on.
    pub friday: Option<String>,
}

/// What the record says about a project, or why it says nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Read {
        stage: Stage,
        /// The gate the record runs for the project, as one shell line.
        gate: Option<String>,
    },
    /// Nothing to read, and why: no rigger, an unknown project, a project
    /// recorded at another path.
    Absent(String),
}

/// Reads the record of the project named `name` whose repository is `root`.
pub trait Record {
    fn read(&self, name: &str, root: &Path) -> Reading;
}

/// The record as rigger keeps it, asked through `rigger ... --json`.
pub struct Rigger;

impl Record for Rigger {
    fn read(&self, name: &str, root: &Path) -> Reading {
        let project = match rigger(&["project", "show", name, "--json"]) {
            Ok(project) => project,
            Err(why) => return Reading::Absent(why),
        };
        // A project of the same name elsewhere is another project: reading
        // its stage would propose someone else's number.
        let recorded = project
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default();
        if !same_place(Path::new(recorded), root) {
            return Reading::Absent(format!(
                "the record keeps `{name}` at another path, not at this repository"
            ));
        }
        let gate = project
            .get("gate")
            .and_then(|g| g.as_str())
            .map(str::to_owned);

        let version = match rigger(&["version", "show", name, "--json"]) {
            Ok(version) => version,
            Err(why) => return Reading::Absent(why),
        };
        match stage(&version) {
            Some(stage) => Reading::Read { stage, gate },
            None => Reading::Absent(format!(
                "the record's stage for `{name}` names no version furca can read"
            )),
        }
    }
}

/// Reads the stage out of `rigger version show --json`, whose shape rigger
/// documents as a contract.
pub(crate) fn stage(json: &serde_json::Value) -> Option<Stage> {
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    let version = stage_version(&text("version")?)?;
    Some(Stage {
        version,
        title: text("title").unwrap_or_default(),
        week: text("week"),
        friday: text("friday"),
    })
}

/// A stage's version as the record spells it: `v0.3.0`, or `v1.15` for a
/// stage titled the short way, which means 1.15.0.
fn stage_version(text: &str) -> Option<Version> {
    let text = text.strip_prefix('v').unwrap_or(text);
    let parts: Vec<&str> = text.split('.').collect();
    let numeric = parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    match parts.len() {
        1 | 2 if numeric => {
            let mut padded = parts.clone();
            padded.resize(3, "0");
            Version::parse(&padded.join(".")).ok()
        }
        _ => Version::parse(text).ok(),
    }
}

fn rigger(args: &[&str]) -> Result<serde_json::Value, String> {
    let output = match Command::new("rigger").args(args).output() {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err("rigger is not installed here".to_owned());
        }
        Err(error) => return Err(format!("rigger did not start: {error}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let why = stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no reason given");
        return Err(format!("rigger {}: {}", args[..2].join(" "), why.trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| {
        format!(
            "rigger {} printed what is not JSON: {e}",
            args[..2].join(" ")
        )
    })
}

/// Whether two paths name the same directory, compared after the file
/// system resolves them - never as text, which differs in case and
/// separators on Windows.
fn same_place(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_is_read_from_the_documented_shape() {
        let json = serde_json::json!({
            "project": "furca",
            "version": "v0.3.0",
            "title": "Release engine: plan",
            "week": "2026-W41",
            "friday": "2026-10-09",
            "status": "planned",
            "tasks": []
        });
        let read = stage(&json).expect("a stage");
        assert_eq!(read.version, Version::new(0, 3, 0));
        assert_eq!(read.friday.as_deref(), Some("2026-10-09"));

        let short = serde_json::json!({ "version": "v1.15", "title": "x" });
        assert_eq!(
            stage(&short).expect("a stage").version,
            Version::new(1, 15, 0)
        );
        assert_eq!(stage_version("v2"), Some(Version::new(2, 0, 0)));
        assert_eq!(
            stage_version("v1.0.0-rc.1"),
            Version::parse("1.0.0-rc.1").ok()
        );
        assert_eq!(stage_version("v1..2"), None);

        let unaimed = serde_json::json!({ "version": "v0.3.0", "title": "x", "week": null });
        assert_eq!(stage(&unaimed).expect("a stage").week, None);
        assert_eq!(stage(&serde_json::json!({ "version": "next" })), None);
    }
}
