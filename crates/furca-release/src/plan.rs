//! The plan: what the next release is, what stands in its way, and what a
//! run will do. Nothing here changes the repository.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use furca_core::Repository;
use semver::Version;
use serde::Serialize;

use crate::commits::{self, Change, Tally};
use crate::config::{Config, Post, Provider, Registry};
use crate::error::Error;
use crate::manifest::{self, Field};
use crate::record::{Reading, Record, Stage};
use crate::registry::Registries;
use crate::texts::{self, Mention};
use crate::version::{self, Bump};

/// How long a dependency update stays fresh. The line updates its whole
/// stack before a new version, at most once a week: an update older than
/// this is due before the release.
const FRESH_FOR_DAYS: i64 = 7;

/// What a plan may ask besides the repository itself. Each source is
/// optional: a plan without one says what it did not ask.
pub struct Sources<'a> {
    pub registries: Option<&'a dyn Registries>,
    pub record: Option<&'a dyn Record>,
    /// The moment the plan is made, for the age of the last dependency
    /// update.
    pub now: SystemTime,
}

/// A repository with a `release.toml`, ready to be planned.
pub struct Release {
    root: PathBuf,
    config: Config,
    repo: Repository,
}

/// What the next release is, what stands in its way and what a run does.
#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub project: String,
    /// The last version released, by tags and registries; `None` before the
    /// first release.
    pub released: Option<Version>,
    pub proposed: Option<Proposal>,
    /// The stage the record says is being built.
    pub stage: Option<Stage>,
    /// Commits since the last release, newest first.
    pub changes: Vec<Change>,
    pub tally: Tally,
    pub checks: Vec<Check>,
    pub steps: Steps,
    /// No check fails.
    pub ready: bool,
}

/// The number the plan proposes, and where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Proposal {
    pub version: Version,
    pub tag: String,
    pub source: Origin,
    /// The step from the last release; `None` for the first one.
    pub bump: Option<Bump>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// The stage the record aims at.
    Record,
    /// The commits since the last release.
    Commits,
    /// The manifests, for a first release.
    Manifests,
}

/// One thing that has to hold before a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    /// A stable identifier: `number`, `commits`, `stack`, `manifests`,
    /// `readme`, `descriptions`, `texts`, `changelog`, `registries`,
    /// `record`.
    pub name: &'static str,
    pub status: Status,
    pub summary: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    /// Worth reading, does not stop the release.
    Warn,
    /// Stops the release.
    Fail,
    /// Not asked: the source was not given.
    Skip,
}

/// What a run does, in order, as far as the plan can tell.
#[derive(Debug, Clone, Serialize)]
pub struct Steps {
    /// Every edit the bump makes, empty when the files already say the new
    /// number.
    pub bump: Vec<Edit>,
    pub changelog: String,
    pub gate: Vec<String>,
    pub commit: Option<String>,
    pub ci: CiStep,
    pub tag: Option<String>,
    pub publish: Vec<Publish>,
    pub post: Vec<Post>,
}

/// One version the bump rewrites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edit {
    pub path: String,
    /// The key in a manifest, or `line N` in a text.
    pub at: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CiStep {
    pub provider: Provider,
    pub workflow: String,
    pub publish: Option<String>,
}

/// A package the tag publishes, and what its registry holds now.
#[derive(Debug, Clone, Serialize)]
pub struct Publish {
    pub registry: Registry,
    pub name: String,
    /// The newest version the registry holds; `None` when it does not hold
    /// the package or was not asked.
    pub latest: Option<Version>,
    /// The registry has never seen the package: the first publish is by
    /// hand, and only then can a tag publish it.
    pub first: bool,
}

impl Release {
    /// Opens the repository containing `path` and reads its `release.toml`.
    pub fn open(path: impl AsRef<Path>) -> Result<Release, Error> {
        let repo = Repository::open(path)?;
        let root = repo
            .workdir()
            .ok_or_else(|| Error::Bare(repo.git_dir().to_path_buf()))?
            .to_path_buf();
        let config = Config::load(&root)?;
        Ok(Release { root, config, repo })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Makes the plan. Asks the registries and the record when `sources`
    /// gives them; changes nothing.
    pub fn plan(&self, sources: &Sources) -> Result<Plan, Error> {
        let config = &self.config;

        let tags: Vec<(Version, String)> = self
            .repo
            .refs()?
            .tags
            .into_iter()
            .filter_map(|tag| Some((version::from_tag(&tag.name)?, tag.name)))
            .collect();
        let released_tag = tags
            .iter()
            .filter(|(v, _)| v.pre.is_empty())
            .max_by(|a, b| a.0.cmp(&b.0))
            .cloned();

        let asked = self.ask_registries(sources.registries);
        let mut taken: BTreeSet<Version> = tags.iter().map(|(v, _)| version::plain(v)).collect();
        for state in &asked {
            if let Ok(Some(versions)) = &state.versions {
                taken.extend(versions.iter().map(version::plain));
            }
        }
        let released = taken.iter().filter(|v| v.pre.is_empty()).max().cloned();

        let changes = self.changes(released_tag.as_ref().map(|(_, tag)| tag.as_str()))?;
        let tally = Tally::of(&changes);
        let needed = commits::needed(&changes);

        let versions = manifest::versions(&self.root, &config.project.manifests)?;
        let written: BTreeSet<&str> = versions.fields.iter().map(|f| f.version.as_str()).collect();
        let written_one = (written.len() == 1)
            .then(|| written.iter().next().and_then(|v| Version::parse(v).ok()))
            .flatten();

        let reading = sources
            .record
            .map(|record| record.read(&config.project.name, &self.root));
        let stage = match &reading {
            Some(Reading::Read { stage, .. }) => Some(stage.clone()),
            _ => None,
        };

        let proposed = propose(
            released.as_ref(),
            needed,
            stage.as_ref(),
            written_one.as_ref(),
        );

        let mut checks = vec![
            number_check(
                released.as_ref(),
                needed,
                proposed.as_ref(),
                &taken,
                &tags,
                &stage,
            ),
            commits_check(
                released_tag.as_ref().map(|(_, t)| t.as_str()),
                &changes,
                &tally,
            ),
            self.stack_check(sources.now)?,
        ];
        let (manifests, bump_fields) =
            self.manifests_check(&versions, released.as_ref(), proposed.as_ref())?;
        checks.push(manifests);
        checks.push(self.readme_check()?);
        checks.push(self.descriptions_check()?);
        let (texts, bump_texts) = self.texts_check(released.as_ref(), proposed.as_ref())?;
        checks.push(texts);
        let bumped = proposed
            .as_ref()
            .is_some_and(|p| written_one.as_ref() == Some(&p.version));
        checks.push(self.changelog_check(released.as_ref(), proposed.as_ref(), bumped)?);
        checks.push(registries_check(
            &asked,
            sources.registries.is_some(),
            released_tag.as_ref(),
        ));
        checks.push(record_check(reading.as_ref(), config));

        let ready = checks.iter().all(|c| c.status != Status::Fail);
        let to = proposed.as_ref().map(|p| p.version.to_string());
        let mut bump = Vec::new();
        if let Some(to) = &to {
            for field in bump_fields {
                bump.push(Edit {
                    at: field.key,
                    path: field.path,
                    from: field.version,
                    to: to.clone(),
                });
            }
            for mention in bump_texts {
                bump.push(Edit {
                    at: format!("line {}", mention.line),
                    to: mention.text.replacen(&mention.version, to, 1),
                    from: mention.text,
                    path: mention.path,
                });
            }
        }

        let steps = Steps {
            bump,
            changelog: config.project.changelog.clone(),
            gate: config.gate.commands.clone(),
            commit: proposed
                .as_ref()
                .map(|p| format!("chore(release): {}", p.tag)),
            ci: CiStep {
                provider: config.ci.provider,
                workflow: config.ci.workflow.clone(),
                publish: config.ci.publish.clone(),
            },
            tag: proposed.as_ref().map(|p| p.tag.clone()),
            publish: asked
                .iter()
                .map(|state| Publish {
                    registry: state.registry,
                    name: state.name.clone(),
                    latest: match &state.versions {
                        Ok(Some(versions)) => versions.iter().max().cloned(),
                        _ => None,
                    },
                    first: matches!(state.versions, Ok(None)),
                })
                .collect(),
            post: config.post.clone(),
        };

        Ok(Plan {
            project: config.project.name.clone(),
            released,
            proposed,
            stage,
            changes,
            tally,
            checks,
            steps,
            ready,
        })
    }

    fn changes(&self, since: Option<&str>) -> Result<Vec<Change>, Error> {
        let mut changes = Vec::new();
        for commit in self.repo.history(since)? {
            let commit = commit?;
            let message = self.repo.message(&commit.id)?;
            changes.push(Change::read(commit.id, commit.subject, &message));
        }
        Ok(changes)
    }

    fn ask_registries(&self, registries: Option<&dyn Registries>) -> Vec<Asked> {
        self.config
            .package
            .iter()
            .map(|package| Asked {
                registry: package.registry,
                name: package.name.clone(),
                versions: match registries {
                    Some(registries) => registries.versions(package.registry, &package.name),
                    None => Err(String::new()),
                },
            })
            .collect()
    }

    fn stack_check(&self, now: SystemTime) -> Result<Check, Error> {
        let now = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        for commit in self.repo.history(None)? {
            let commit = commit?;
            let change = Change::read(commit.id.clone(), commit.subject.clone(), &commit.subject);
            if !change.updates_dependencies() {
                continue;
            }
            let days = (now - commit.committer.seconds).max(0) / 86_400;
            let date = commit.committer.time.get(..10).unwrap_or_default();
            let at = format!("{} {} on {date}", short(&commit.id), commit.subject);
            return Ok(if days >= FRESH_FOR_DAYS {
                check(
                    "stack",
                    Status::Fail,
                    format!(
                        "the dependencies were last updated {days} days ago; update them first with one `chore(deps)` commit"
                    ),
                    vec![at],
                )
            } else {
                check(
                    "stack",
                    Status::Pass,
                    format!("updated {}", days_ago(days)),
                    vec![at],
                )
            });
        }
        Ok(check(
            "stack",
            Status::Fail,
            "no `chore(deps)` commit in the history: the stack has never been updated".to_owned(),
            Vec::new(),
        ))
    }

    /// The manifests agree on one number and it is the last release or the
    /// next one; every package's manifest is among the files the bump
    /// rewrites. Returns the fields the bump rewrites.
    fn manifests_check(
        &self,
        versions: &manifest::Versions,
        released: Option<&Version>,
        proposed: Option<&Proposal>,
    ) -> Result<(Check, Vec<Field>), Error> {
        let mut details = versions.problems.clone();
        let mut bump = Vec::new();
        let written: BTreeSet<&str> = versions.fields.iter().map(|f| f.version.as_str()).collect();
        let summary;

        if written.len() > 1 {
            for field in &versions.fields {
                details.push(format!(
                    "{} `{}` = {}",
                    field.path, field.key, field.version
                ));
            }
            summary = format!("the manifests disagree: {}", join_set(&written));
        } else if let Some(one) = written.iter().next() {
            let one = Version::parse(one).ok();
            let next = proposed.map(|p| &p.version);
            if one.is_some() && one.as_ref() == next {
                summary = format!(
                    "{} fields already say {}",
                    versions.fields.len(),
                    next.map(ToString::to_string).unwrap_or_default()
                );
            } else if one.is_some() && one.as_ref() == released {
                bump = versions.fields.clone();
                summary = format!(
                    "{} fields say {}, the bump rewrites them",
                    versions.fields.len(),
                    released.map(ToString::to_string).unwrap_or_default()
                );
            } else {
                summary = format!(
                    "the manifests say {}, which is neither the last release ({}) nor the next ({})",
                    written.iter().next().copied().unwrap_or_default(),
                    released.map_or("none".to_owned(), ToString::to_string),
                    next.map_or("none".to_owned(), ToString::to_string),
                );
                details.push(summary.clone());
            }
        } else {
            summary = "no manifest carries a version".to_owned();
            details.push(summary.clone());
        }

        for package in &self.config.package {
            let meta = manifest::meta(&self.root, package)?;
            if meta.name.as_deref() != Some(package.name.as_str()) {
                details.push(format!(
                    "`{}` names the package {}, release.toml calls it `{}`",
                    package.manifest,
                    meta.name.map_or("nothing".to_owned(), |n| format!("`{n}`")),
                    package.name
                ));
            }
            if !meta.publishable {
                details.push(format!(
                    "`{}` forbids publishing `{}`",
                    package.manifest, package.name
                ));
            }
            let covered = versions.fields.iter().any(|f| {
                (f.path == package.manifest && (f.key == "package.version" || f.key == "version"))
                    || (meta.inherits_version && f.key == "workspace.package.version")
            });
            if !covered {
                details.push(format!(
                    "`{}` publishes `{}` but its version is not among the manifests the bump rewrites",
                    package.manifest, package.name
                ));
            }
        }

        let status = if details.is_empty() {
            Status::Pass
        } else {
            Status::Fail
        };
        Ok((check("manifests", status, summary, details), bump))
    }

    /// One README for every shop window: the root one. No published
    /// package's directory keeps its own (an example app may), every crate
    /// points at the root one, and its links work off GitHub.
    fn readme_check(&self) -> Result<Check, Error> {
        let mut details = Vec::new();
        let readme = self.root.join("README.md");
        let text = match std::fs::read_to_string(&readme) {
            Ok(text) => text,
            Err(_) => {
                return Ok(check(
                    "readme",
                    Status::Fail,
                    "there is no README.md at the root".to_owned(),
                    Vec::new(),
                ));
            }
        };

        let tracked = self.repo.tracked_paths()?;
        let package_dirs: BTreeSet<&str> = self
            .config
            .package
            .iter()
            .filter_map(|p| p.manifest.rsplit_once('/').map(|(dir, _)| dir))
            .collect();
        for path in &tracked {
            let Some((dir, name)) = path.rsplit_once('/') else {
                continue;
            };
            if name.to_ascii_lowercase().starts_with("readme") && package_dirs.contains(dir) {
                details.push(format!(
                    "`{path}` is a second README; the root one is the only source"
                ));
            }
        }

        for package in &self.config.package {
            if package.registry != Registry::Crates {
                continue;
            }
            let meta = manifest::meta(&self.root, package)?;
            match meta.readme.as_deref() {
                Some("README.md") => {}
                Some(other) => details.push(format!(
                    "`{}` shows `{other}` on crates.io, not the root README",
                    package.name
                )),
                None => details.push(format!(
                    "`{}` names no readme, so its crates.io page has none",
                    package.name
                )),
            }
        }

        for (number, line) in text.lines().enumerate() {
            for target in link_targets(line) {
                let absolute = target.starts_with("http") || target.starts_with('#');
                if !absolute && !target.is_empty() {
                    details.push(format!(
                        "README.md:{}: `{target}` is relative and breaks on crates.io and npm",
                        number + 1
                    ));
                }
            }
        }

        Ok(if details.is_empty() {
            check(
                "readme",
                Status::Pass,
                "one README, its links absolute".to_owned(),
                Vec::new(),
            )
        } else {
            check(
                "readme",
                Status::Fail,
                format!("{} problems with the README", details.len()),
                details,
            )
        })
    }

    /// Every package says what it is, and a wrapper says it in the words of
    /// the package it wraps.
    fn descriptions_check(&self) -> Result<Check, Error> {
        let mut details = Vec::new();
        let mut pairs = 0;
        for package in &self.config.package {
            let meta = manifest::meta(&self.root, package)?;
            let Some(description) = meta.description.filter(|d| !d.trim().is_empty()) else {
                details.push(format!("`{}` has no description", package.name));
                continue;
            };
            let Some(other) = self.config.wrapped(package) else {
                continue;
            };
            let wrapped = &other.name;
            pairs += 1;
            let theirs = manifest::meta(&self.root, other)?
                .description
                .unwrap_or_default();
            if theirs != description {
                details.push(format!(
                    "`{}` and `{wrapped}` describe the product differently: \"{description}\" and \"{theirs}\"",
                    package.name
                ));
            }
        }
        Ok(if details.is_empty() {
            let summary = match pairs {
                0 => format!(
                    "{} describe themselves",
                    plural(self.config.package.len(), "package")
                ),
                1 => "the wrapper says what it wraps says".to_owned(),
                n => format!("{n} wrappers say what they wrap says"),
            };
            check("descriptions", Status::Pass, summary, Vec::new())
        } else {
            check(
                "descriptions",
                Status::Fail,
                format!("{} description problems", details.len()),
                details,
            )
        })
    }

    /// Versions written into texts: each one is the last release (the bump
    /// rewrites it), the next one, a planned later one, or history. Anything
    /// else is stale. Returns the mentions the bump rewrites.
    fn texts_check(
        &self,
        released: Option<&Version>,
        proposed: Option<&Proposal>,
    ) -> Result<(Check, Vec<Mention>), Error> {
        let mut names = vec![self.config.project.name.clone()];
        names.extend(self.config.package.iter().map(|p| p.name.clone()));
        names.sort_by_key(|n| std::cmp::Reverse(n.len()));
        names.dedup();

        let next = proposed.map(|p| &p.version);
        let (mut bump, mut stale, mut current, mut ahead, mut history) =
            (Vec::new(), Vec::new(), 0, Vec::new(), 0);
        let mut unclosed = Vec::new();
        for path in self.repo.tracked_paths()? {
            if self.config.is_as_written(&path) || !texts::scanned(&path) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(self.root.join(&path)) else {
                continue;
            };
            let scan = texts::scan(&path, &text, &names);
            if let Some(line) = scan.unclosed {
                unclosed.push(format!(
                    "{path}:{line}: a historical-versions block opens here and never closes"
                ));
            }
            for mention in scan.mentions {
                let Ok(version) = Version::parse(&mention.version) else {
                    continue;
                };
                if mention.history {
                    history += 1;
                } else if Some(&version) == next {
                    current += 1;
                } else if Some(&version) == released {
                    bump.push(mention);
                } else if next.or(released).is_some_and(|edge| &version > edge) {
                    ahead.push(mention);
                } else {
                    stale.push(mention);
                }
            }
        }

        let place = |m: &Mention| format!("{}:{}: {}", m.path, m.line, m.text);
        let mut parts = Vec::new();
        for (count, one, many) in [
            (bump.len(), "follows the bump", "follow the bump"),
            (
                current,
                "already says the next version",
                "already say the next version",
            ),
            (ahead.len(), "names a later version", "name a later version"),
            (history, "is history", "are history"),
        ] {
            match count {
                0 => {}
                1 => parts.push(format!("1 {one}")),
                n => parts.push(format!("{n} {many}")),
            }
        }
        let check = if stale.is_empty() && unclosed.is_empty() {
            let summary = if parts.is_empty() {
                "no version written in the texts".to_owned()
            } else {
                parts.join(", ")
            };
            check(
                "texts",
                Status::Pass,
                summary,
                ahead
                    .iter()
                    .map(|m| format!("later: {}", place(m)))
                    .collect(),
            )
        } else {
            let mut details = unclosed;
            details.extend(stale.iter().map(place));
            check(
                "texts",
                Status::Fail,
                if stale.is_empty() {
                    "a historical-versions block never closes".to_owned()
                } else {
                    format!(
                        "{} stale versions in the texts: write the shape vX.Y.Z, or mark history with <!-- historical versions -->",
                        stale.len()
                    )
                },
                details,
            )
        };
        Ok((check, bump))
    }

    /// The changelog gets one section per release. A section for the new
    /// number is the release's own once the manifests carry that number too
    /// (the release edits are made and wait for their commit); before that
    /// it is a duplicate the run would write twice.
    fn changelog_check(
        &self,
        released: Option<&Version>,
        proposed: Option<&Proposal>,
        bumped: bool,
    ) -> Result<Check, Error> {
        let path = self.root.join(&self.config.project.changelog);
        let text = std::fs::read_to_string(&path).map_err(|source| Error::Read {
            path: path.clone(),
            source,
        })?;
        let sections: BTreeSet<String> = text
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .filter_map(|heading| {
                let number = heading
                    .trim_start_matches('[')
                    .split([']', ' '])
                    .next()?
                    .trim_start_matches('v');
                Version::parse(number).ok().map(|v| v.to_string())
            })
            .collect();
        let changelog = &self.config.project.changelog;
        if let Some(next) = proposed.map(|p| p.version.to_string())
            && sections.contains(&next)
        {
            return Ok(if bumped {
                check(
                    "changelog",
                    Status::Pass,
                    format!(
                        "{changelog} has its section for {next}, as the manifests have the number"
                    ),
                    Vec::new(),
                )
            } else {
                check(
                    "changelog",
                    Status::Fail,
                    format!(
                        "{changelog} already has a section for {next}, while the manifests do not carry it yet"
                    ),
                    Vec::new(),
                )
            });
        }
        if let Some(last) = released.map(ToString::to_string)
            && !sections.contains(&last)
        {
            return Ok(check(
                "changelog",
                Status::Warn,
                format!("{changelog} has no section for {last}, the last release"),
                Vec::new(),
            ));
        }
        Ok(check(
            "changelog",
            Status::Pass,
            format!("the new section goes under the header of {changelog}"),
            Vec::new(),
        ))
    }
}

/// What one registry said about one package.
struct Asked {
    registry: Registry,
    name: String,
    /// `Err("")` when no registry was asked at all.
    versions: Result<Option<Vec<Version>>, String>,
}

/// The number: from the record when it has a stage, else from the commits,
/// else (first release) from the manifests.
fn propose(
    released: Option<&Version>,
    needed: Option<Bump>,
    stage: Option<&Stage>,
    written: Option<&Version>,
) -> Option<Proposal> {
    let (version, source) = if let Some(stage) = stage {
        (stage.version.clone(), Origin::Record)
    } else if let Some(released) = released {
        (needed?.apply(released), Origin::Commits)
    } else {
        (written?.clone(), Origin::Manifests)
    };
    Some(Proposal {
        tag: version::tag(&version),
        bump: released.and_then(|r| Bump::between(r, &version)),
        version,
        source,
    })
}

fn number_check(
    released: Option<&Version>,
    needed: Option<Bump>,
    proposed: Option<&Proposal>,
    taken: &BTreeSet<Version>,
    tags: &[(Version, String)],
    stage: &Option<Stage>,
) -> Check {
    let Some(proposed) = proposed else {
        return check(
            "number",
            Status::Fail,
            "nothing to propose: no commits since the last release and no stage in the record"
                .to_owned(),
            Vec::new(),
        );
    };
    let next = &proposed.version;
    let mut details = Vec::new();

    if taken.contains(&version::plain(next)) {
        let by_tag = tags.iter().any(|(v, _)| v == next);
        return check(
            "number",
            Status::Fail,
            format!(
                "{} is taken {}",
                proposed.tag,
                if by_tag { "by a tag" } else { "in a registry" }
            ),
            if stage.is_some() {
                vec!["the record's stage names a released number; rename the stage".to_owned()]
            } else {
                Vec::new()
            },
        );
    }

    let Some(released) = released else {
        return check(
            "number",
            Status::Pass,
            format!("{} is the first release", proposed.tag),
            Vec::new(),
        );
    };
    let Some(step) = proposed.bump else {
        let free = [Bump::Patch, Bump::Minor, Bump::Major]
            .map(|b| version::tag(&exact_step(b, released)))
            .join(", ");
        return check(
            "number",
            Status::Fail,
            if next > released {
                format!(
                    "{} skips a number after v{released}; the next free are {free}",
                    proposed.tag
                )
            } else {
                format!("{} is below the last release, v{released}", proposed.tag)
            },
            if stage.is_some() {
                vec![
                    "the number is the next free one, not the stage's: rename the stage".to_owned(),
                ]
            } else {
                Vec::new()
            },
        );
    };
    if let Some(needed) = needed
        && !step.covers(needed, released)
    {
        return check(
            "number",
            Status::Fail,
            format!(
                "the commits carry a {} change, more than {} allows after v{released}; {} at least",
                bump_word(needed),
                proposed.tag,
                version::tag(&needed.apply(released))
            ),
            Vec::new(),
        );
    }
    if proposed.source == Origin::Record
        && let Some(needed) = needed
    {
        let from_commits = needed.apply(released);
        if &from_commits != next {
            details.push(format!(
                "the commits alone would give {}",
                version::tag(&from_commits)
            ));
        }
    }
    check(
        "number",
        Status::Pass,
        format!(
            "{} is free: a {} step after v{released}",
            proposed.tag,
            bump_word(step)
        ),
        details,
    )
}

fn commits_check(since: Option<&str>, changes: &[Change], tally: &Tally) -> Check {
    let since = since.map_or("the start".to_owned(), ToOwned::to_owned);
    if changes.is_empty() {
        return check(
            "commits",
            Status::Fail,
            format!("nothing has been committed since {since}"),
            Vec::new(),
        );
    }
    let mut parts = Vec::new();
    for (count, word) in [
        (tally.breaking, "breaking"),
        (tally.features, "features"),
        (tally.fixes, "fixes"),
        (tally.other, "other"),
        (tally.unconventional, "outside the convention"),
    ] {
        if count > 0 {
            parts.push(format!("{count} {word}"));
        }
    }
    let summary = format!(
        "{} since {since}: {}",
        plural(tally.total, "commit"),
        parts.join(", ")
    );
    let odd: Vec<String> = changes
        .iter()
        .filter(|c| !c.conventional())
        .map(|c| format!("{} {}", short(&c.id), c.subject))
        .collect();
    if odd.is_empty() {
        check("commits", Status::Pass, summary, Vec::new())
    } else {
        check("commits", Status::Warn, summary, odd)
    }
}

fn registries_check(
    asked: &[Asked],
    consulted: bool,
    released_tag: Option<&(Version, String)>,
) -> Check {
    if asked.is_empty() {
        return check(
            "registries",
            Status::Pass,
            "nothing is published to a registry".to_owned(),
            Vec::new(),
        );
    }
    if !consulted {
        return check(
            "registries",
            Status::Skip,
            "the registries were not asked".to_owned(),
            Vec::new(),
        );
    }
    let mut status = Status::Pass;
    let mut details = Vec::new();
    let (mut unasked, mut new, mut untagged) = (Vec::new(), Vec::new(), Vec::new());
    for state in asked {
        let label = format!("{} `{}`", state.registry.label(), state.name);
        match &state.versions {
            Err(why) => {
                status = Status::Fail;
                unasked.push(state.name.as_str());
                details.push(format!("{label}: {why}"));
            }
            Ok(None) => {
                if status == Status::Pass {
                    status = Status::Warn;
                }
                new.push(state.name.as_str());
                details.push(format!(
                    "{label} is not published yet: the first publish is by hand, and only then can a tag publish it"
                ));
            }
            Ok(Some(versions)) => {
                let latest = versions.iter().filter(|v| v.pre.is_empty()).max();
                match (latest, released_tag) {
                    (Some(latest), Some((tagged, _))) if latest > tagged => {
                        if status == Status::Pass {
                            status = Status::Warn;
                        }
                        untagged.push(state.name.as_str());
                        details.push(format!(
                            "{label} holds {latest}, which has no tag; the newest tag is v{tagged}"
                        ));
                    }
                    (Some(latest), _) => details.push(format!("{label}: {latest}")),
                    (None, _) => details.push(format!("{label}: no release yet")),
                }
            }
        }
    }
    let summary = if !unasked.is_empty() {
        format!(
            "could not ask about {}, so the number is not proven free",
            unasked.join(", ")
        )
    } else if !new.is_empty() {
        let verb = if new.len() == 1 { "is" } else { "are" };
        format!(
            "{} {verb} not published yet: the first publish is by hand",
            new.join(", ")
        )
    } else if !untagged.is_empty() {
        let verb = if untagged.len() == 1 { "holds" } else { "hold" };
        format!("{} {verb} a release with no tag", untagged.join(", "))
    } else {
        format!("{} asked", plural(asked.len(), "package"))
    };
    check("registries", status, summary, details)
}

fn record_check(reading: Option<&Reading>, config: &Config) -> Check {
    match reading {
        None => check(
            "record",
            Status::Skip,
            "the record was not asked; the number comes from the commits".to_owned(),
            Vec::new(),
        ),
        Some(Reading::Absent(why)) => check(
            "record",
            Status::Warn,
            format!("no stage from the record ({why}); the number comes from the commits"),
            Vec::new(),
        ),
        Some(Reading::Read { stage, gate }) => {
            let due = match (&stage.friday, &stage.week) {
                (Some(friday), _) => format!("due {friday}"),
                (None, Some(week)) => format!("aimed at {week}"),
                (None, None) => "not aimed at a week yet".to_owned(),
            };
            let summary = format!("stage v{} \"{}\", {due}", stage.version, stage.title);
            let ours = config.gate.commands.join(" && ");
            match gate {
                Some(theirs) if theirs.trim() != ours => check(
                    "record",
                    Status::Warn,
                    summary,
                    vec![
                        "the record runs another gate than release.toml; one of them is stale"
                            .to_owned(),
                        format!("record: {theirs}"),
                        format!("release.toml: {ours}"),
                    ],
                ),
                _ => check("record", Status::Pass, summary, Vec::new()),
            }
        }
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

/// The version one step of `bump` after `from`, as the numbers go - unlike
/// [`Bump::apply`], a major step before 1.0 is 1.0.0.
fn exact_step(bump: Bump, from: &Version) -> Version {
    match bump {
        Bump::Major => Version::new(from.major + 1, 0, 0),
        Bump::Minor => Version::new(from.major, from.minor + 1, 0),
        Bump::Patch => Version::new(from.major, from.minor, from.patch + 1),
    }
}

fn bump_word(bump: Bump) -> &'static str {
    match bump {
        Bump::Major => "major",
        Bump::Minor => "minor",
        Bump::Patch => "patch",
    }
}

fn days_ago(days: i64) -> String {
    match days {
        0 => "today".to_owned(),
        1 => "yesterday".to_owned(),
        n => format!("{n} days ago"),
    }
}

fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        format!("1 {word}")
    } else {
        format!("{count} {word}s")
    }
}

fn short(id: &str) -> &str {
    &id[..id.len().min(7)]
}

fn join_set(set: &BTreeSet<&str>) -> String {
    set.iter().copied().collect::<Vec<_>>().join(", ")
}

/// Link and image targets on one Markdown or HTML line.
fn link_targets(line: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    for (marker, end) in [("](", ')'), ("src=\"", '"'), ("href=\"", '"')] {
        let mut rest = line;
        while let Some(at) = rest.find(marker) {
            let target = &rest[at + marker.len()..];
            let target = &target[..target.find(end).unwrap_or(target.len())];
            // `[text](url "title")`: the title is not part of the target.
            targets.push(target.split_whitespace().next().unwrap_or_default());
            rest = &rest[at + marker.len()..];
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn stage(version: &str) -> Stage {
        Stage {
            version: v(version),
            title: "A stage".to_owned(),
            week: None,
            friday: None,
        }
    }

    #[test]
    fn the_record_names_the_number_when_it_has_a_stage() {
        let p = propose(
            Some(&v("0.2.0")),
            Some(Bump::Patch),
            Some(&stage("0.3.0")),
            None,
        )
        .expect("a proposal");
        assert_eq!(
            (p.version, p.source, p.bump),
            (v("0.3.0"), Origin::Record, Some(Bump::Minor))
        );
        let p = propose(Some(&v("0.2.0")), Some(Bump::Patch), None, None).expect("a proposal");
        assert_eq!((p.version, p.source), (v("0.2.1"), Origin::Commits));
        let p = propose(None, Some(Bump::Minor), None, Some(&v("0.1.0"))).expect("a proposal");
        assert_eq!(
            (p.version, p.source, p.bump),
            (v("0.1.0"), Origin::Manifests, None)
        );
        assert!(propose(Some(&v("0.2.0")), None, None, None).is_none());
    }

    fn number(released: &str, needed: Option<Bump>, stage_version: Option<&str>) -> Check {
        let stage = stage_version.map(stage);
        let proposed = propose(Some(&v(released)), needed, stage.as_ref(), None);
        let taken: BTreeSet<Version> = [v(released)].into();
        let tags = vec![(v(released), format!("v{released}"))];
        number_check(
            Some(&v(released)),
            needed,
            proposed.as_ref(),
            &taken,
            &tags,
            &stage,
        )
    }

    #[test]
    fn a_stage_that_skips_a_number_is_refused() {
        let check = number("0.5.0", Some(Bump::Minor), Some("0.7.0"));
        assert_eq!(check.status, Status::Fail, "{check:?}");
        assert!(check.summary.contains("v0.6.0"), "{}", check.summary);
    }

    #[test]
    fn a_stage_that_names_a_released_number_is_refused() {
        let check = number("0.5.0", Some(Bump::Minor), Some("0.5.0"));
        assert_eq!(check.status, Status::Fail);
        assert!(check.summary.contains("taken"), "{}", check.summary);
    }

    #[test]
    fn a_stage_too_small_for_its_commits_is_refused() {
        let check = number("1.4.0", Some(Bump::Minor), Some("1.4.1"));
        assert_eq!(check.status, Status::Fail, "{check:?}");
        assert!(check.summary.contains("v1.5.0"), "{}", check.summary);
        // Before 1.0 a minor step carries a breaking change.
        assert_eq!(
            number("0.4.0", Some(Bump::Major), Some("0.5.0")).status,
            Status::Pass
        );
    }

    #[test]
    fn a_stage_may_step_further_than_its_commits_need() {
        let check = number("0.2.0", Some(Bump::Patch), Some("0.3.0"));
        assert_eq!(check.status, Status::Pass, "{check:?}");
        assert_eq!(check.details, ["the commits alone would give v0.2.1"]);
    }

    #[test]
    fn link_targets_include_images_and_drop_titles() {
        let line = r#"<img src="assets/a.svg"> [x](https://a.b "t") [y](#z) <a href="b.md">"#;
        let mut found = link_targets(line);
        found.sort_unstable();
        assert_eq!(found, ["#z", "assets/a.svg", "b.md", "https://a.b"]);
    }
}
