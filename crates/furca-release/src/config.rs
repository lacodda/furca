//! `release.toml`: what a repository releases, where its version is written,
//! what has to pass first and what happens after the tag.
//!
//! The file is the repository's own and public, so it holds nothing private:
//! no machine paths, no stand names, no tokens. Anything that belongs to the
//! owner's machine (which stand a deploy goes to, where the record lives) is
//! asked of the tool that keeps it.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// The file every release reads, at the root of the repository.
pub const FILE: &str = "release.toml";

/// A parsed and checked `release.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub project: Project,
    pub gate: Gate,
    pub ci: Ci,
    /// What the release publishes, one entry per package per registry.
    #[serde(default)]
    pub package: Vec<Package>,
    /// The files the tag's release page carries.
    pub page: Option<PageAssets>,
    /// What happens after the publish: the install, the deploy.
    #[serde(default)]
    pub post: Post,
}

/// The files a release page has to carry, by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageAssets {
    /// File names, with `{tag}` and `{version}` filled in for each release:
    /// `furca-{tag}-x86_64-pc-windows-msvc.zip`.
    pub assets: Vec<String>,
}

impl PageAssets {
    /// The names for one release.
    pub fn names(&self, tag: &str, version: &semver::Version) -> Vec<String> {
        self.assets
            .iter()
            .map(|a| {
                a.replace("{tag}", tag)
                    .replace("{version}", &version.to_string())
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// The product's name: the record knows the project by it, and it is the
    /// word its own output prints before a version (`furca 0.3.0`).
    pub name: String,
    /// The changelog the release adds a section to.
    pub changelog: String,
    /// Every file that carries the version, relative to the root. A Cargo
    /// workspace root covers its members and the versions of the path
    /// dependencies between them.
    pub manifests: Vec<String>,
    /// Texts whose versions are left as written: records of what happened,
    /// such as `docs/adr/`, and examples that show numbers of their own. A
    /// directory ends with `/`. The changelog is always one and need not be
    /// listed.
    #[serde(default)]
    pub as_written: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    /// Shell commands run from the root, in order; the first that fails stops
    /// the release before anything is committed.
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ci {
    pub provider: Provider,
    /// The workflow that has to be green on the release commit before it is
    /// tagged, as a file name in the provider's workflow directory.
    pub workflow: String,
    /// The workflow a tag starts to publish the packages, if there is one.
    pub publish: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    Forgejo,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Provider::Github => "GitHub",
            Provider::Forgejo => "Forgejo",
        }
    }

    /// Where the provider looks for workflow files.
    pub fn workflow_dir(self) -> &'static str {
        match self {
            Provider::Github => ".github/workflows",
            Provider::Forgejo => ".forgejo/workflows",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub registry: Registry,
    /// The name the registry knows the package by.
    pub name: String,
    /// The manifest that describes the package, relative to the root.
    pub manifest: String,
    /// The package this one ships the binary of, such as an npm wrapper
    /// around a crate's CLI: a package of another registry, which may have
    /// the same name. The two describe the product in the same words.
    pub wraps: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Registry {
    Crates,
    Npm,
}

impl Registry {
    pub fn label(self) -> &'static str {
        match self {
            Registry::Crates => "crates.io",
            Registry::Npm => "npm",
        }
    }
}

/// The steps after the publish, each at most once and always in this
/// order: the install, then the deploy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Post {
    pub install: Option<Install>,
    pub deploy: Option<Deploy>,
}

/// Install the released build for the owner with the repository's own
/// installers, `tools/install.ps1` and `tools/install.sh`, and prove it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Install {
    /// The program the installers put on the PATH; the project's name when
    /// left out. Its upper-cased name prefixes the installers' environment:
    /// `FURCA_VERSION`.
    pub bin: Option<String>,
    /// The other names the installers make for it, such as `tn` for
    /// `turnout`: each has to be the same build.
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// Deploy with turnout from the repository: which stand it goes to is
/// turnout's own configuration, not this file's, and turnout checks the
/// stand's health.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deploy {}

impl Config {
    /// Reads and checks `release.toml` at `root`.
    ///
    /// Every problem found is reported at once, so one edit can fix them all.
    pub fn load(root: &Path) -> Result<Self, Error> {
        let path = root.join(FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::NoConfig(root.to_path_buf()));
            }
            Err(source) => return Err(Error::Read { path, source }),
        };
        let config: Config =
            toml::from_str(&text).map_err(|e| Error::Config(vec![e.message().to_owned()]))?;
        let problems = config.problems(root);
        if problems.is_empty() {
            Ok(config)
        } else {
            Err(Error::Config(problems))
        }
    }

    fn problems(&self, root: &Path) -> Vec<String> {
        let mut problems = Vec::new();
        let mut missing = |what: &str, path: &str| {
            if !root.join(path).is_file() {
                problems.push(format!("{what} `{path}` does not exist"));
            }
        };

        missing("the changelog", &self.project.changelog);
        for manifest in &self.project.manifests {
            missing("the manifest", manifest);
        }
        for package in &self.package {
            missing("the manifest of package", &package.manifest);
        }
        let dir = self.ci.provider.workflow_dir();
        missing("the CI workflow", &format!("{dir}/{}", self.ci.workflow));
        if let Some(publish) = &self.ci.publish {
            missing("the publish workflow", &format!("{dir}/{publish}"));
        }

        if self.project.name.trim().is_empty() {
            problems.push("`project.name` is empty".to_owned());
        }
        if self.project.manifests.is_empty() {
            problems.push(
                "`project.manifests` names no file, so nothing carries the version".to_owned(),
            );
        }
        for manifest in &self.project.manifests {
            if crate::manifest::Kind::of(manifest).is_none() {
                problems.push(format!(
                    "furca does not know where `{manifest}` keeps its version; a manifest is a Cargo.toml or a .json file"
                ));
            }
        }
        if self.gate.commands.iter().all(|c| c.trim().is_empty()) {
            problems.push(
                "`gate.commands` is empty, so nothing would stop a broken release".to_owned(),
            );
        }
        for entry in &self.project.as_written {
            if !root.join(entry).exists() {
                problems.push(format!("the `as_written` entry `{entry}` does not exist"));
            } else if root.join(entry).is_dir() != entry.ends_with('/') {
                problems.push(format!(
                    "the `as_written` entry `{entry}` must end with `/` exactly when it is a directory"
                ));
            }
        }

        if let Some(page) = &self.page {
            if page.assets.is_empty() {
                problems.push("`page.assets` names no file".to_owned());
            }
            for asset in &page.assets {
                let unknown = asset
                    .replace("{tag}", "")
                    .replace("{version}", "")
                    .contains(['{', '}']);
                if unknown {
                    problems.push(format!(
                        "the asset `{asset}` has a placeholder other than {{tag}} and {{version}}"
                    ));
                }
            }
        }
        if let Some(install) = &self.post.install {
            for installer in crate::install::INSTALLERS {
                if !root.join(installer).is_file() {
                    problems.push(format!("the installer `{installer}` does not exist"));
                }
            }
            let bin = self.bin(install);
            let named = |name: &str| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            };
            if !named(bin) {
                problems.push(format!(
                    "`post.install.bin` `{bin}` is not a program's name"
                ));
            }
            for alias in &install.aliases {
                if !named(alias) || alias == bin {
                    problems.push(format!(
                        "`post.install.aliases` has `{alias}`, which is not another program's name"
                    ));
                }
            }
        }

        let mut seen = BTreeSet::new();
        for package in &self.package {
            if !seen.insert((package.registry, package.name.as_str())) {
                problems.push(format!(
                    "{} `{}` is listed twice",
                    package.registry.label(),
                    package.name
                ));
            }
            if let Some(wrapped) = &package.wraps
                && self.wrapped(package).is_none()
            {
                problems.push(format!(
                    "{} `{}` wraps `{wrapped}`, which is not a package of another registry here",
                    package.registry.label(),
                    package.name
                ));
            }
        }
        problems
    }

    /// The program `[post.install]` installs: its `bin`, or the project's
    /// name.
    pub fn bin<'a>(&'a self, install: &'a Install) -> &'a str {
        install.bin.as_deref().unwrap_or(&self.project.name)
    }

    /// The package `package` wraps: the one of that name in another
    /// registry, so that a crate and its npm wrapper may share a name.
    pub fn wrapped(&self, package: &Package) -> Option<&Package> {
        let name = package.wraps.as_ref()?;
        self.package
            .iter()
            .find(|p| &p.name == name && p.registry != package.registry)
    }

    /// Whether the versions in the text at `path` are left as written: the
    /// changelog, or a file under an entry of `project.as_written`.
    pub fn is_as_written(&self, path: &str) -> bool {
        path == self.project.changelog
            || self.project.as_written.iter().any(|entry| {
                if entry.ends_with('/') {
                    path.starts_with(entry.as_str())
                } else {
                    path == entry
                }
            })
    }
}
