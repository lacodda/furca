//! The shop windows of a release that went out: what a stranger sees after
//! the tag. The release page is out with every file `release.toml` lists,
//! every package's registry holds the version, and every link the manifests
//! show answers. A run checks them after its publish; `furca release
//! verify` checks them for any tag.

use std::path::Path;

use semver::Version;
use serde::Serialize;

use crate::config::Registry;
use crate::error::Error;
use crate::hosting::Forge;
use crate::install::{self, InstallTo, Machine, Order};
use crate::manifest;
use crate::plan::{Check, Release, Status};
use crate::registry::Registries;
use crate::version;

/// Asks whether a web address answers.
pub trait Web {
    /// The HTTP status `url` finally answers with, redirects followed; `Err`
    /// when it does not answer at all.
    fn status(&self, url: &str) -> Result<u16, String>;
}

/// What a verification asks besides the repository.
pub struct Shops<'a> {
    pub registries: &'a dyn Registries,
    pub web: &'a dyn Web,
    pub forge: &'a dyn Forge,
}

/// The shop windows of one release, check by check.
#[derive(Debug, Clone, Serialize)]
pub struct Verified {
    pub project: String,
    pub tag: String,
    pub version: Version,
    /// `page`, `registries`, `links`; with an install check, also
    /// `installer`, `found`, `version`, `build` and `aliases`.
    pub checks: Vec<Check>,
    /// No check fails.
    pub ok: bool,
}

impl Release {
    /// The newest release tag, `None` before the first release.
    pub fn released_tag(&self) -> Result<Option<String>, Error> {
        Ok(self
            .repo
            .refs()?
            .tags
            .into_iter()
            .filter_map(|tag| Some((version::from_tag(&tag.name)?, tag.name)))
            .filter(|(v, _)| v.pre.is_empty())
            .max_by(|a, b| a.0.cmp(&b.0))
            .map(|(_, tag)| tag))
    }

    /// Checks the shop windows of the release `tag`: the page, the
    /// registries, the links.
    pub fn verify(&self, tag: &str, shops: &Shops) -> Result<Verified, Error> {
        let version = version::from_tag(tag)
            .ok_or_else(|| Error::Config(vec![format!("`{tag}` is not a release tag (vX.Y.Z)")]))?;
        let checks = vec![
            self.page_check(tag, &version, shops.forge),
            self.registries_held(&version, shops.registries),
            self.links_check(shops.web)?,
        ];
        Ok(Verified {
            project: self.config.project.name.clone(),
            tag: tag.to_owned(),
            ok: checks.iter().all(|c| c.status != Status::Fail),
            version,
            checks,
        })
    }

    /// Installs `tag` with the repository's own installer into `dir` -
    /// never the owner's place - and proves what it put there, as a run's
    /// `install` step does. `Err` when release.toml has no `[post.install]`.
    pub fn install_check(
        &self,
        tag: &str,
        dir: &Path,
        forge: &dyn Forge,
        machine: &dyn Machine,
        say: &mut dyn FnMut(&str),
    ) -> Result<Vec<Check>, Error> {
        let version = version::from_tag(tag)
            .ok_or_else(|| Error::Config(vec![format!("`{tag}` is not a release tag (vX.Y.Z)")]))?;
        let post = self.config.post.install.as_ref().ok_or_else(|| {
            Error::Config(vec![
                "`[post.install]` is missing, so there is no installer to check".to_owned(),
            ])
        })?;
        let bin = self.config.bin(post);
        Ok(install::install(
            &Order {
                root: &self.root,
                bin,
                aliases: &post.aliases,
                tag,
                version: &version,
                into: InstallTo::Dir(dir),
            },
            forge,
            machine,
            say,
        ))
    }

    fn page_check(&self, tag: &str, version: &Version, forge: &dyn Forge) -> Check {
        let page = match forge.page(tag) {
            Ok(Some(page)) => page,
            Ok(None) => {
                return check(
                    "page",
                    Status::Fail,
                    format!("{tag} has no release page"),
                    Vec::new(),
                );
            }
            Err(why) => return check("page", Status::Fail, why, Vec::new()),
        };
        if page.draft {
            return check(
                "page",
                Status::Fail,
                format!("the release page of {tag} is still a draft"),
                vec![page.url],
            );
        }
        let Some(listed) = &self.config.page else {
            return check(
                "page",
                Status::Pass,
                format!("out with {} files", page.assets.len()),
                vec![
                    page.url,
                    "release.toml lists no `[page] assets`, so the files are not checked"
                        .to_owned(),
                ],
            );
        };
        let missing: Vec<String> = listed
            .names(tag, version)
            .into_iter()
            .filter(|name| !page.assets.contains(name))
            .map(|name| format!("missing: {name}"))
            .collect();
        if missing.is_empty() {
            check(
                "page",
                Status::Pass,
                format!(
                    "out with every file release.toml lists ({} of {})",
                    listed.assets.len(),
                    page.assets.len()
                ),
                vec![page.url],
            )
        } else {
            let mut details = missing;
            details.push(page.url);
            check(
                "page",
                Status::Fail,
                format!("the release page of {tag} lacks files release.toml lists"),
                details,
            )
        }
    }

    /// Every package's registry holds `version`.
    pub(crate) fn registries_held(&self, version: &Version, registries: &dyn Registries) -> Check {
        if self.config.package.is_empty() {
            return check(
                "registries",
                Status::Pass,
                "nothing is published to a registry".to_owned(),
                Vec::new(),
            );
        }
        let mut details = Vec::new();
        let mut failed = false;
        for package in &self.config.package {
            let label = format!("{} `{}`", package.registry.label(), package.name);
            match registries.versions(package.registry, &package.name) {
                Ok(Some(held)) if held.contains(version) => {
                    details.push(format!("{label}: {version}"));
                }
                Ok(Some(held)) => {
                    failed = true;
                    let latest = held
                        .iter()
                        .max()
                        .map_or("nothing".to_owned(), ToString::to_string);
                    details.push(format!(
                        "{label} does not hold {version}; its newest is {latest}"
                    ));
                }
                Ok(None) => {
                    failed = true;
                    details.push(format!(
                        "{label} was never published: the first publish is by hand, {}",
                        first_publish(package.registry, &package.name)
                    ));
                }
                Err(why) => {
                    failed = true;
                    details.push(format!("{label}: {why}"));
                }
            }
        }
        let mut labels: Vec<&str> = Vec::new();
        for package in &self.config.package {
            if !labels.contains(&package.registry.label()) {
                labels.push(package.registry.label());
            }
        }
        if failed {
            check(
                "registries",
                Status::Fail,
                format!("{version} is not in every registry"),
                details,
            )
        } else {
            check(
                "registries",
                Status::Pass,
                format!("{version} is in {}", labels.join(", ")),
                details,
            )
        }
    }

    /// Every homepage, documentation and repository link the manifests
    /// show answers.
    fn links_check(&self, web: &dyn Web) -> Result<Check, Error> {
        let mut paths: Vec<&str> = self
            .config
            .project
            .manifests
            .iter()
            .map(String::as_str)
            .collect();
        paths.extend(self.config.package.iter().map(|p| p.manifest.as_str()));
        let mut links: Vec<String> = Vec::new();
        for path in paths {
            for link in manifest::links(&self.root, path)? {
                if !links.contains(&link) {
                    links.push(link);
                }
            }
        }
        if links.is_empty() {
            return Ok(check(
                "links",
                Status::Warn,
                "the manifests name no homepage, documentation or repository".to_owned(),
                Vec::new(),
            ));
        }
        let mut details = Vec::new();
        let mut dead = 0;
        for link in &links {
            match web.status(link) {
                Ok(status) if (200..400).contains(&status) => {
                    details.push(format!("{link}: {status}"))
                }
                Ok(status) => {
                    dead += 1;
                    details.push(format!("{link} answers {status}"));
                }
                Err(why) => {
                    dead += 1;
                    details.push(format!("{link}: {why}"));
                }
            }
        }
        Ok(if dead == 0 {
            check(
                "links",
                Status::Pass,
                format!("{} links answer", links.len()),
                details,
            )
        } else {
            check(
                "links",
                Status::Fail,
                format!("{dead} of {} links do not answer", links.len()),
                details,
            )
        })
    }
}

/// How a package its registry has never seen is published the first time.
pub(crate) fn first_publish(registry: Registry, name: &str) -> String {
    match registry {
        Registry::Crates => format!("`cargo publish -p {name}`"),
        Registry::Npm => "`npm publish` in its directory".to_owned(),
    }
}

fn check(name: &'static str, status: Status, summary: String, details: Vec<String>) -> Check {
    Check {
        name,
        status,
        summary,
        details,
    }
}
