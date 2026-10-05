//! Where a repository writes its version, and what its packages say about
//! themselves.
//!
//! Read only: rewriting (the bump) keeps each file's formatting and is the
//! next release's work. What is read here is exactly what the bump will
//! touch, so the plan can show it.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::config::{Package, Registry};
use crate::error::Error;

/// The kinds of manifest the engine can read a version from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A `Cargo.toml`: a package, a workspace root, or both.
    Cargo,
    /// A JSON file with a top-level `"version"`: `package.json`,
    /// `tauri.conf.json`.
    Json,
}

impl Kind {
    pub fn of(path: &str) -> Option<Kind> {
        let name = path.rsplit('/').next().unwrap_or(path);
        if name == "Cargo.toml" {
            Some(Kind::Cargo)
        } else if name.ends_with(".json") {
            Some(Kind::Json)
        } else {
            None
        }
    }
}

/// One place a version is written: a file and the key inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// The file, relative to the root, with `/` separators.
    pub path: String,
    /// The key, dotted: `workspace.package.version`,
    /// `dependencies.furca-core.version`, `version`.
    pub key: String,
    /// The value as written.
    pub version: String,
}

/// What a set of manifests says, and what could not be read from it.
#[derive(Debug, Default)]
pub struct Versions {
    pub fields: Vec<Field>,
    pub problems: Vec<String>,
}

/// Reads every version field the manifests hold. A Cargo workspace root
/// brings in its members and the path dependencies between them.
pub fn versions(root: &Path, manifests: &[String]) -> Result<Versions, Error> {
    let mut out = Versions::default();
    for manifest in manifests {
        match Kind::of(manifest) {
            Some(Kind::Cargo) => cargo_versions(root, manifest, &mut out)?,
            Some(Kind::Json) => {
                let value = read_json(root, manifest)?;
                match value.get("version").and_then(|v| v.as_str()) {
                    Some(version) => out.fields.push(Field {
                        path: manifest.clone(),
                        key: "version".to_owned(),
                        version: version.to_owned(),
                    }),
                    None => out
                        .problems
                        .push(format!("`{manifest}` has no top-level \"version\"")),
                }
            }
            // Config::load has already refused these.
            None => {}
        }
    }
    Ok(out)
}

fn cargo_versions(root: &Path, manifest: &str, out: &mut Versions) -> Result<(), Error> {
    let table = read_toml(root, manifest)?;
    if let Some(version) = string_at(&table, &["workspace", "package", "version"]) {
        out.fields
            .push(field(manifest, "workspace.package.version", version));
    }
    if let Some(version) = string_at(&table, &["package", "version"]) {
        out.fields.push(field(manifest, "package.version", version));
    }

    let base = parent(manifest);
    let members = members(root, &base, &table, &mut out.problems)?;
    let mut tables = vec![(manifest.to_owned(), table)];
    for member in &members {
        let path = join(&base, member, "Cargo.toml");
        if path == manifest {
            continue;
        }
        let member_table = read_toml(root, &path)?;
        if let Some(version) = string_at(&member_table, &["package", "version"]) {
            out.fields.push(field(&path, "package.version", version));
        }
        tables.push((path, member_table));
    }

    // A path dependency on a member names the member's version as well; a
    // path dependency outside the workspace is someone else's number.
    let member_dirs: Vec<String> = members.iter().map(|m| join(&base, m, "")).collect();
    for (path, table) in &tables {
        let dir = parent(path);
        for (key, dependency) in dependencies(table) {
            let Some(dep_path) = dependency.get("path").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(version) = dependency.get("version").and_then(|v| v.as_str()) else {
                continue;
            };
            if member_dirs.contains(&join(&dir, dep_path, "")) {
                out.fields.push(field(path, &key, version));
            }
        }
    }
    Ok(())
}

/// The workspace members of a root manifest, relative to its directory.
fn members(
    root: &Path,
    base: &str,
    table: &toml::Table,
    problems: &mut Vec<String>,
) -> Result<Vec<String>, Error> {
    let Some(workspace) = table.get("workspace").and_then(|w| w.as_table()) else {
        return Ok(Vec::new());
    };
    let list = |key: &str| -> Vec<String> {
        workspace
            .get(key)
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let exclude = list("exclude");
    let mut members = Vec::new();
    for entry in list("members") {
        if let Some(prefix) = entry.strip_suffix("/*") {
            let dir = root.join(join(base, prefix, ""));
            let read = std::fs::read_dir(&dir).map_err(|source| Error::Read {
                path: dir.clone(),
                source,
            })?;
            let mut found: Vec<String> = read
                .flatten()
                .filter(|e| e.path().join("Cargo.toml").is_file())
                .filter_map(|e| e.file_name().to_str().map(|n| format!("{prefix}/{n}")))
                .collect();
            found.sort();
            members.extend(found);
        } else if entry.contains(['*', '?', '[']) {
            problems.push(format!(
                "the workspace member pattern `{entry}` is not read: furca reads `*` only as a whole last component"
            ));
        } else {
            members.push(entry);
        }
    }
    members.retain(|m| !exclude.contains(m));
    Ok(members)
}

/// Every dependency table of a manifest, with the dotted key of its
/// `version`: `dependencies.<name>.version`, and the same under
/// `dev-dependencies`, `build-dependencies`, `workspace.dependencies` and
/// each `target.<cfg>`.
fn dependencies(table: &toml::Table) -> Vec<(String, &toml::Table)> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    fn collect<'t>(
        out: &mut Vec<(String, &'t toml::Table)>,
        prefix: String,
        deps: Option<&'t toml::Value>,
    ) {
        let Some(deps) = deps.and_then(|d| d.as_table()) else {
            return;
        };
        for (name, value) in deps {
            if let Some(dependency) = value.as_table() {
                out.push((format!("{prefix}.{name}.version"), dependency));
            }
        }
    }
    let mut out = Vec::new();
    for kind in KINDS {
        collect(&mut out, kind.to_owned(), table.get(kind));
    }
    if let Some(workspace) = table.get("workspace").and_then(|w| w.as_table()) {
        collect(
            &mut out,
            "workspace.dependencies".to_owned(),
            workspace.get("dependencies"),
        );
    }
    if let Some(targets) = table.get("target").and_then(|t| t.as_table()) {
        for (cfg, target) in targets {
            for kind in KINDS {
                collect(&mut out, format!("target.{cfg}.{kind}"), target.get(kind));
            }
        }
    }
    out
}

/// What a package's manifest says about it, as its registry will show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub name: Option<String>,
    pub description: Option<String>,
    /// The README the registry page renders, relative to the root; `None`
    /// when the manifest names none.
    pub readme: Option<String>,
    /// Whether the manifest allows publishing at all.
    pub publishable: bool,
    /// The version is `version.workspace = true`: the workspace root
    /// carries it.
    pub inherits_version: bool,
}

/// Reads the metadata of `package` from its manifest, following
/// `*.workspace = true` to the workspace root at `root/Cargo.toml`.
pub fn meta(root: &Path, package: &Package) -> Result<Meta, Error> {
    match package.registry {
        Registry::Crates => {
            let table = read_toml(root, &package.manifest)?;
            let workspace = if root.join("Cargo.toml").is_file() {
                read_toml(root, "Cargo.toml")?
            } else {
                toml::Table::new()
            };
            let inherited = |key: &str| -> Option<&toml::Value> {
                let own = table.get("package")?.get(key)?;
                if own.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                    workspace.get("workspace")?.get("package")?.get(key)
                } else {
                    Some(own)
                }
            };
            let dir = parent(&package.manifest);
            let readme = match table.get("package").and_then(|p| p.get("readme")) {
                Some(own) if own.get("workspace").and_then(|w| w.as_bool()) == Some(true) => {
                    inherited("readme")
                        .and_then(|v| v.as_str())
                        .map(|p| join("", p, ""))
                }
                Some(own) => own.as_str().map(|p| join(&dir, p, "")),
                None => None,
            };
            let publishable = match table.get("package").and_then(|p| p.get("publish")) {
                Some(toml::Value::Boolean(allowed)) => *allowed,
                Some(toml::Value::Array(registries)) => !registries.is_empty(),
                _ => true,
            };
            Ok(Meta {
                name: string_at(&table, &["package", "name"]).map(str::to_owned),
                description: inherited("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned),
                readme,
                publishable,
                inherits_version: table
                    .get("package")
                    .and_then(|p| p.get("version"))
                    .and_then(|v| v.get("workspace"))
                    .and_then(|w| w.as_bool())
                    == Some(true),
            })
        }
        Registry::Npm => {
            let value = read_json(root, &package.manifest)?;
            let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned);
            Ok(Meta {
                name: text("name"),
                description: text("description"),
                readme: None,
                publishable: value.get("private").and_then(|v| v.as_bool()) != Some(true),
                inherits_version: false,
            })
        }
    }
}

fn field(path: &str, key: &str, version: &str) -> Field {
    Field {
        path: path.to_owned(),
        key: key.to_owned(),
        version: version.to_owned(),
    }
}

fn read_toml(root: &Path, path: &str) -> Result<toml::Table, Error> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full).map_err(|source| Error::Read {
        path: full.clone(),
        source,
    })?;
    text.parse::<toml::Table>()
        .map_err(|e| Error::Config(vec![format!("`{path}` is not valid TOML: {}", e.message())]))
}

fn read_json(root: &Path, path: &str) -> Result<serde_json::Value, Error> {
    let full = root.join(path);
    let text = std::fs::read_to_string(&full).map_err(|source| Error::Read {
        path: full.clone(),
        source,
    })?;
    serde_json::from_str(&text)
        .map_err(|e| Error::Config(vec![format!("`{path}` is not valid JSON: {e}")]))
}

fn string_at<'t>(table: &'t toml::Table, keys: &[&str]) -> Option<&'t str> {
    let (first, rest) = keys.split_first()?;
    let mut value = table.get(*first)?;
    for key in rest {
        value = value.get(*key)?;
    }
    value.as_str()
}

/// The directory of a root-relative path, `""` for the root itself.
fn parent(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| dir.to_owned())
        .unwrap_or_default()
}

/// `base/relative/file`, normalised lexically to a root-relative path with
/// `/` separators: `crates/furca-cli` + `../furca-core` is
/// `crates/furca-core`. Paths are compared as components, never as text.
pub(crate) fn join(base: &str, relative: &str, file: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let joined: PathBuf = Path::new(base).join(relative).join(file);
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => {}
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::{Kind, join};

    #[test]
    fn a_relative_path_is_resolved_against_its_base() {
        assert_eq!(
            join("crates/furca-cli", "../furca-core", ""),
            "crates/furca-core"
        );
        assert_eq!(join("crates/furca-cli", "../../README.md", ""), "README.md");
        assert_eq!(join("", "crates/*", "Cargo.toml"), "crates/*/Cargo.toml");
        assert_eq!(
            join("a", r"b\c", ""),
            if cfg!(windows) { "a/b/c" } else { r"a/b\c" }
        );
    }

    #[test]
    fn the_kind_of_a_manifest_is_its_file_name() {
        assert_eq!(Kind::of("Cargo.toml"), Some(Kind::Cargo));
        assert_eq!(Kind::of("crates/x/Cargo.toml"), Some(Kind::Cargo));
        assert_eq!(Kind::of("npm/package.json"), Some(Kind::Json));
        assert_eq!(Kind::of("pyproject.toml"), None);
    }
}
