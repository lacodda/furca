//! The owner's record of the project: which version the current stage aims
//! at, under what title and for which Friday - and, once a release is out,
//! the word that it went out. The record is rigger's; furca asks it and
//! tells it through its CLI.
//!
//! The record is optional. Without rigger on the machine, or without the
//! project in it, the plan proposes a number from the commits alone and the
//! run writes nothing, and both say so.
//!
//! The record also keeps the list of the owner's projects, which is where
//! `furca line` finds the repositories to read when it is named none.

use std::path::{Path, PathBuf};
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

/// A release as the run saw it go out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shipped {
    pub tag: String,
    /// A release page went out with its files.
    pub release: bool,
    /// The registries that hold the version, by label: `crates.io`, `npm`.
    pub registries: Vec<String>,
    /// One line on how it went.
    pub text: String,
}

/// A repository the record keeps, under its project's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Kept {
    pub name: String,
    pub path: PathBuf,
}

/// The record of the project named `name` whose repository is `root`.
pub trait Record {
    fn read(&self, name: &str, root: &Path) -> Reading;

    /// Writes that `shipped` went out. `Err` says why nothing was written:
    /// no rigger here, a project the record does not keep at `root`.
    fn shipped(&self, name: &str, root: &Path, shipped: &Shipped) -> Result<(), String>;
}

/// The record as rigger keeps it, asked through `rigger ... --json`.
pub struct Rigger;

impl Record for Rigger {
    fn read(&self, name: &str, root: &Path) -> Reading {
        let project = match self.here(name, root) {
            Ok(project) => project,
            Err(why) => return Reading::Absent(why),
        };
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

    fn shipped(&self, name: &str, root: &Path, shipped: &Shipped) -> Result<(), String> {
        self.here(name, root)?;
        let mut args = vec![
            "note",
            name,
            "--kind",
            "shipped",
            "--tag",
            shipped.tag.as_str(),
            "--json",
        ];
        if shipped.release {
            args.push("--release");
        }
        for registry in &shipped.registries {
            args.extend(["--registry", registry.as_str()]);
        }
        if !shipped.text.is_empty() {
            args.push(shipped.text.as_str());
        }
        rigger(&args).map(drop)
    }
}

impl Rigger {
    /// The repositories of the projects the record keeps, in its order.
    /// `Err` says why there is no list: no rigger here, or one that did not
    /// answer.
    pub fn repositories(&self) -> Result<Vec<Kept>, String> {
        kept(&rigger(&["project", "list", "--json"])?)
    }

    fn here(&self, name: &str, root: &Path) -> Result<serde_json::Value, String> {
        let project = rigger(&["project", "show", name, "--json"])?;
        // A project of the same name elsewhere is another project: reading
        // its stage would propose someone else's number, and writing to it
        // would ship someone else's release.
        let recorded = project
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or_default();
        if same_place(Path::new(recorded), root) {
            Ok(project)
        } else {
            Err(format!(
                "the record keeps `{name}` at another path, not at this repository"
            ))
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

/// Reads the repositories out of `rigger project list --json`. A project
/// that is a folder rather than a repository, one with no path, and one the
/// owner retired are not part of the line's repositories.
pub(crate) fn kept(json: &serde_json::Value) -> Result<Vec<Kept>, String> {
    let projects = json
        .as_array()
        .ok_or("rigger project list printed something other than a list")?;
    Ok(projects
        .iter()
        .filter(|p| p.get("kind").and_then(|k| k.as_str()) == Some("repo"))
        .filter(|p| p.get("retired_at").is_none_or(serde_json::Value::is_null))
        .filter_map(|p| {
            let name = p.get("name")?.as_str()?;
            let path = p.get("path")?.as_str().filter(|path| !path.is_empty())?;
            Some(Kept {
                name: name.to_owned(),
                path: PathBuf::from(path),
            })
        })
        .collect())
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

    #[test]
    fn the_line_is_the_repositories_the_record_keeps_and_has_not_retired() {
        let json = serde_json::json!([
            { "name": "app", "path": "/src/app", "kind": "repo", "retired_at": null },
            { "name": "old", "path": "/src/old", "kind": "repo", "retired_at": "2026-10-13T17:41:26Z" },
            { "name": "notes", "path": "/notes", "kind": "folder", "retired_at": null },
            { "name": "place", "path": null, "kind": "repo", "retired_at": null },
            { "name": "lib", "path": "/src/lib", "kind": "repo" }
        ]);
        let kept = kept(&json).expect("a list");
        let names: Vec<&str> = kept.iter().map(|k| k.name.as_str()).collect();
        assert_eq!(names, ["app", "lib"]);
        assert_eq!(kept[0].path, PathBuf::from("/src/app"));

        assert!(super::kept(&serde_json::json!({ "error": "x" })).is_err());
    }
}
