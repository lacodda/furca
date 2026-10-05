//! The plan on repositories built by `git` in a temp directory, with the
//! registries and the record played by fakes: no network, no rigger, no
//! repository of the author's.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use furca_release::{
    Error, Origin, Plan, Reading, Record, Registries, Registry, Release, Sources, Stage, Status,
};
use semver::Version;

/// The moment every plan here is made at, and the day commits are dated
/// against.
const NOW: u64 = 1_790_000_000;
const DAY: u64 = 86_400;

struct Repo {
    dir: tempfile::TempDir,
}

impl Repo {
    /// A released project: one crate and its npm wrapper at 0.2.0, tagged,
    /// with a dependency update a day old and a feature and a fix since.
    fn released() -> Repo {
        let repo = Repo::empty();
        repo.write("Cargo.toml", CARGO);
        repo.write("npm/package.json", NPM);
        repo.write("README.md", README);
        repo.write(
            "CHANGELOG.md",
            "# Changelog\n\n## [0.2.0] - 2026-09-01\n\n- First\n",
        );
        repo.write(".github/workflows/ci.yml", "name: CI\n");
        repo.write(".github/workflows/publish.yml", "name: Publish\n");
        repo.write("docs/adr/0001-a.md", "Since v0.1.0 the CLI prints JSON.\n");
        repo.write("release.toml", RELEASE);
        repo.commit("chore: found the project", NOW - 30 * DAY);
        repo.git(&["tag", "-a", "v0.2.0", "-m", "v0.2.0"], NOW - 30 * DAY);
        repo.commit("chore(deps): update the toolchain", NOW - DAY);
        repo.commit("feat: add a command", NOW - DAY);
        repo.commit("fix: keep the order\n\nThe body says why.", NOW - DAY);
        repo
    }

    fn empty() -> Repo {
        let repo = Repo {
            dir: tempfile::tempdir().expect("temp dir"),
        };
        repo.git(&["init", "--quiet", "--initial-branch=main"], NOW);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn write(&self, path: &str, text: &str) {
        let full = self.path().join(path);
        std::fs::create_dir_all(full.parent().expect("a parent")).expect("dirs");
        std::fs::write(full, text).expect("write");
    }

    fn commit(&self, message: &str, at: u64) {
        self.git(&["add", "--all"], at);
        self.git(&["commit", "--quiet", "--allow-empty", "-m", message], at);
    }

    fn git(&self, args: &[&str], at: u64) {
        let empty = self.path().join(".git-no-config");
        std::fs::write(&empty, "").expect("empty config");
        let date = format!("{at} +0000");
        let output = Command::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", &empty)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Author")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Author")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn plan(&self, registries: &dyn Registries, record: Option<&dyn Record>) -> Plan {
        Release::open(self.path())
            .expect("the repository opens")
            .plan(&Sources {
                registries: Some(registries),
                record,
                now: UNIX_EPOCH + Duration::from_secs(NOW),
            })
            .expect("a plan")
    }
}

const CARGO: &str = r#"[package]
name = "demo"
version = "0.2.0"
edition = "2024"
description = "A demo that does one thing."
readme = "README.md"
"#;

const NPM: &str = r#"{
  "name": "demo-cli",
  "version": "0.2.0",
  "demo": { "binary": "v0.2.0" },
  "description": "A demo that does one thing."
}
"#;

const README: &str = r#"# demo

![banner](https://example.com/banner.svg)

```console
$ demo --version
demo 0.2.0
```

Pin a release with `DEMO_VERSION=v0.2.0`.
"#;

const RELEASE: &str = r#"[project]
name = "demo"
changelog = "CHANGELOG.md"
manifests = ["Cargo.toml", "npm/package.json"]
as_written = ["docs/adr/"]

[gate]
commands = ["cargo test"]

[ci]
provider = "github"
workflow = "ci.yml"
publish = "publish.yml"

[[package]]
registry = "crates"
name = "demo"
manifest = "Cargo.toml"

[[package]]
registry = "npm"
name = "demo-cli"
manifest = "npm/package.json"
wraps = "demo"

[[post]]
kind = "install"
"#;

/// Registries that hold what the test says, and count the questions.
struct Held {
    crates: Result<Option<Vec<&'static str>>, &'static str>,
    npm: Result<Option<Vec<&'static str>>, &'static str>,
    asked: RefCell<usize>,
}

impl Held {
    fn released() -> Held {
        Held {
            crates: Ok(Some(vec!["0.1.0", "0.2.0"])),
            npm: Ok(Some(vec!["0.1.0", "0.2.0"])),
            asked: RefCell::new(0),
        }
    }
}

impl Registries for Held {
    fn versions(&self, registry: Registry, _name: &str) -> Result<Option<Vec<Version>>, String> {
        *self.asked.borrow_mut() += 1;
        let held = match registry {
            Registry::Crates => &self.crates,
            Registry::Npm => &self.npm,
        };
        match held {
            Ok(Some(versions)) => Ok(Some(
                versions
                    .iter()
                    .map(|v| Version::parse(v).unwrap())
                    .collect(),
            )),
            Ok(None) => Ok(None),
            Err(why) => Err((*why).to_owned()),
        }
    }
}

/// A record whose stage is whatever the test says.
struct Aimed(&'static str);

impl Record for Aimed {
    fn read(&self, _name: &str, _root: &Path) -> Reading {
        Reading::Read {
            stage: Stage {
                version: Version::parse(self.0).unwrap(),
                title: "Demo stage".to_owned(),
                week: Some("2026-W40".to_owned()),
                friday: Some("2026-10-02".to_owned()),
            },
            gate: Some("cargo test".to_owned()),
        }
    }
}

fn status(plan: &Plan, name: &str) -> Status {
    find(plan, name).status
}

fn find<'p>(plan: &'p Plan, name: &str) -> &'p furca_release::Check {
    plan.checks
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no check `{name}` in {:?}", plan.checks))
}

fn failing(plan: &Plan) -> Vec<&'static str> {
    plan.checks
        .iter()
        .filter(|c| c.status == Status::Fail)
        .map(|c| c.name)
        .collect()
}

#[test]
fn a_feature_since_the_tag_proposes_the_next_minor_and_is_ready() {
    let repo = Repo::released();
    let held = Held::released();
    let plan = repo.plan(&held, None);

    assert!(plan.ready, "{:#?}", plan.checks);
    assert_eq!(plan.released, Some(Version::new(0, 2, 0)));
    let proposed = plan.proposed.as_ref().expect("a proposal");
    assert_eq!(proposed.version, Version::new(0, 3, 0));
    assert_eq!(proposed.source, Origin::Commits);
    assert_eq!(proposed.tag, "v0.3.0");
    assert_eq!(plan.tally.total, 3);
    assert_eq!((plan.tally.features, plan.tally.fixes), (1, 1));
    assert_eq!(status(&plan, "record"), Status::Skip);
    assert_eq!(*held.asked.borrow(), 2, "one question per package");

    // The bump rewrites both manifests and every mention of the last
    // release outside the history.
    let edits: Vec<(String, String, String)> = plan
        .steps
        .bump
        .iter()
        .map(|e| (e.path.clone(), e.at.clone(), e.to.clone()))
        .collect();
    for expected in [
        ("Cargo.toml", "package.version", "0.3.0"),
        ("npm/package.json", "version", "0.3.0"),
        ("npm/package.json", "line 4", "v0.3.0"),
        ("README.md", "line 7", "demo 0.3.0"),
        ("README.md", "line 10", "v0.3.0"),
    ] {
        assert!(
            edits
                .iter()
                .any(|(p, a, t)| (p.as_str(), a.as_str(), t.as_str()) == expected),
            "no edit {expected:?} in {edits:#?}"
        );
    }
    assert!(
        !edits.iter().any(|(p, _, _)| p.starts_with("docs/adr/")),
        "the ADRs are history: {edits:#?}"
    );
    assert_eq!(plan.steps.commit.as_deref(), Some("chore(release): v0.3.0"));
    assert_eq!(plan.steps.gate, ["cargo test"]);
}

#[test]
fn only_fixes_since_the_tag_propose_a_patch() {
    let repo = Repo::released();
    repo.git(&["tag", "-a", "v0.3.0", "-m", "v0.3.0"], NOW - DAY);
    repo.write("Cargo.toml", &CARGO.replace("0.2.0", "0.3.0"));
    repo.write("npm/package.json", &NPM.replace("0.2.0", "0.3.0"));
    repo.write("README.md", &README.replace("0.2.0", "0.3.0"));
    repo.write(
        "CHANGELOG.md",
        "# Changelog\n\n## [0.3.0] - 2026-09-20\n\n## [0.2.0] - 2026-09-01\n",
    );
    repo.commit("fix: one more thing", NOW - DAY + 60);
    let held = Held {
        crates: Ok(Some(vec!["0.2.0", "0.3.0"])),
        npm: Ok(Some(vec!["0.2.0", "0.3.0"])),
        asked: RefCell::new(0),
    };

    let plan = repo.plan(&held, None);
    assert!(plan.ready, "{:#?}", plan.checks);
    assert_eq!(
        plan.proposed.map(|p| p.version),
        Some(Version::new(0, 3, 1))
    );
}

#[test]
fn the_record_names_the_number_and_a_hole_is_refused() {
    let repo = Repo::released();
    let held = Held::released();

    let plan = repo.plan(&held, Some(&Aimed("0.3.0")));
    assert!(plan.ready, "{:#?}", plan.checks);
    assert_eq!(
        plan.proposed.as_ref().map(|p| p.source),
        Some(Origin::Record)
    );
    assert_eq!(status(&plan, "record"), Status::Pass);

    let plan = repo.plan(&held, Some(&Aimed("0.4.0")));
    assert_eq!(failing(&plan), ["number"], "{:#?}", plan.checks);
    assert!(
        find(&plan, "number").summary.contains("v0.3.0"),
        "the next free number is named: {:?}",
        find(&plan, "number")
    );

    let plan = repo.plan(&held, Some(&Aimed("0.2.1")));
    assert_eq!(
        failing(&plan),
        ["number"],
        "a feature needs more than a patch"
    );
}

#[test]
fn a_number_a_registry_has_seen_is_taken_even_without_a_tag() {
    let repo = Repo::released();
    let held = Held {
        crates: Ok(Some(vec!["0.1.0", "0.2.0", "0.3.0"])),
        npm: Ok(Some(vec!["0.1.0", "0.2.0"])),
        asked: RefCell::new(0),
    };
    let plan = repo.plan(&held, Some(&Aimed("0.3.0")));
    assert!(failing(&plan).contains(&"number"), "{:#?}", plan.checks);
    assert_eq!(status(&plan, "registries"), Status::Warn);
}

#[test]
fn a_registry_that_cannot_be_asked_leaves_the_number_unproven() {
    let repo = Repo::released();
    let held = Held {
        crates: Err("crates.io did not answer: timed out"),
        npm: Ok(Some(vec!["0.2.0"])),
        asked: RefCell::new(0),
    };
    let plan = repo.plan(&held, None);
    assert_eq!(failing(&plan), ["registries"]);
    assert!(!plan.ready);
}

#[test]
fn a_package_no_registry_has_seen_is_published_by_hand_first() {
    let repo = Repo::released();
    let held = Held {
        crates: Ok(Some(vec!["0.2.0"])),
        npm: Ok(None),
        asked: RefCell::new(0),
    };
    let plan = repo.plan(&held, None);
    assert!(
        plan.ready,
        "a first publish is not a failure: {:#?}",
        plan.checks
    );
    assert_eq!(status(&plan, "registries"), Status::Warn);
    let npm = plan
        .steps
        .publish
        .iter()
        .find(|p| p.name == "demo-cli")
        .expect("the wrapper is published");
    assert!(npm.first);
}

#[test]
fn a_stale_version_in_a_text_stops_the_release_and_history_does_not() {
    let repo = Repo::released();
    repo.write(
        "docs/guide.md",
        "Install with DEMO_VERSION=v0.1.0.\n\
         <!-- historical versions -->\n\
         Since v0.1.0 it prints JSON.\n\
         <!-- /historical versions -->\n\
         The window arrives in v0.9.0. Pin any tag, vX.Y.Z.\n",
    );
    repo.commit("docs: a guide", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);

    assert_eq!(failing(&plan), ["texts"], "{:#?}", plan.checks);
    let texts = find(&plan, "texts");
    assert_eq!(texts.details, ["docs/guide.md:1: v0.1.0"], "{texts:?}");

    repo.write(
        "docs/guide.md",
        "<!-- historical versions -->\nSince v0.1.0.\n",
    );
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(
        find(&plan, "texts").details,
        ["docs/guide.md:1: a historical-versions block opens here and never closes"]
    );
}

#[test]
fn a_text_listed_as_written_is_not_read() {
    let repo = Repo::released();
    // Stale by every rule - if it were read.
    repo.write("docs/examples.md", "app 0.0.7 -> 0.0.8, tagged v0.0.7\n");
    repo.write(
        "release.toml",
        &RELEASE.replace(
            r#"as_written = ["docs/adr/"]"#,
            r#"as_written = ["docs/adr/", "docs/examples.md"]"#,
        ),
    );
    repo.commit("docs: examples", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);
    assert!(plan.ready, "{:#?}", plan.checks);
}

#[test]
fn an_old_dependency_update_stops_the_release() {
    let repo = Repo::released();
    let plan = Release::open(repo.path())
        .unwrap()
        .plan(&Sources {
            registries: Some(&Held::released()),
            record: None,
            now: UNIX_EPOCH + Duration::from_secs(NOW + 7 * DAY),
        })
        .unwrap();
    assert_eq!(failing(&plan), ["stack"], "{:#?}", plan.checks);
    assert!(find(&plan, "stack").summary.contains("8 days"));
}

#[test]
fn manifests_that_disagree_stop_the_release() {
    let repo = Repo::released();
    repo.write(
        "npm/package.json",
        &NPM.replace("\"version\": \"0.2.0\"", "\"version\": \"0.1.9\""),
    );
    repo.commit("fix: forget the wrapper", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(failing(&plan), ["manifests"], "{:#?}", plan.checks);
    assert!(
        plan.steps.bump.iter().all(|e| e.at.starts_with("line")),
        "no field is bumped while they disagree"
    );
}

#[test]
fn a_wrapper_that_describes_the_product_otherwise_is_caught() {
    let repo = Repo::released();
    repo.write(
        "npm/package.json",
        &NPM.replace("does one thing", "does two things"),
    );
    repo.commit("docs: reword the wrapper", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(failing(&plan), ["descriptions"], "{:#?}", plan.checks);
}

#[test]
fn a_second_readme_and_a_relative_link_are_caught() {
    let repo = Repo::released();
    repo.write("npm/README.md", "# demo\n");
    repo.write(
        "README.md",
        &format!("{README}\n![logo](assets/logo.svg)\n"),
    );
    repo.commit("docs: a second readme", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(failing(&plan), ["readme"], "{:#?}", plan.checks);
    let readme = find(&plan, "readme");
    assert_eq!(readme.details.len(), 2, "{readme:?}");
}

#[test]
fn nothing_since_the_tag_is_nothing_to_release() {
    let repo = Repo::empty();
    repo.write("Cargo.toml", CARGO);
    repo.write("npm/package.json", NPM);
    repo.write("README.md", README);
    repo.write("CHANGELOG.md", "# Changelog\n\n## [0.2.0] - 2026-09-01\n");
    repo.write(".github/workflows/ci.yml", "name: CI\n");
    repo.write(".github/workflows/publish.yml", "name: Publish\n");
    repo.write("docs/adr/0001-a.md", "x\n");
    repo.write("release.toml", RELEASE);
    repo.commit("chore(deps): update", NOW - DAY);
    repo.git(&["tag", "v0.2.0"], NOW - DAY);
    let plan = repo.plan(&Held::released(), None);
    assert!(failing(&plan).contains(&"commits"), "{:#?}", plan.checks);
    assert!(failing(&plan).contains(&"number"), "{:#?}", plan.checks);
}

#[test]
fn a_first_release_takes_the_number_the_manifests_carry() {
    let repo = Repo::empty();
    for (path, text) in [
        ("Cargo.toml", CARGO.replace("0.2.0", "0.1.0")),
        ("npm/package.json", NPM.replace("0.2.0", "0.1.0")),
        ("README.md", README.replace("0.2.0", "0.1.0")),
        ("CHANGELOG.md", "# Changelog\n".to_owned()),
        (".github/workflows/ci.yml", "name: CI\n".to_owned()),
        (
            ".github/workflows/publish.yml",
            "name: Publish\n".to_owned(),
        ),
        ("docs/adr/0001-a.md", "x\n".to_owned()),
        ("release.toml", RELEASE.to_owned()),
    ] {
        repo.write(path, &text);
    }
    repo.commit("chore(deps): pin the toolchain", NOW - DAY);
    repo.commit("feat: the first command", NOW - DAY);
    let held = Held {
        crates: Ok(None),
        npm: Ok(None),
        asked: RefCell::new(0),
    };
    let plan = repo.plan(&held, None);
    assert!(plan.ready, "{:#?}", plan.checks);
    let proposed = plan.proposed.expect("a proposal");
    assert_eq!(
        (proposed.version, proposed.source, proposed.bump),
        (Version::new(0, 1, 0), Origin::Manifests, None)
    );
    assert!(plan.steps.bump.is_empty(), "the files already say 0.1.0");
}

#[test]
fn a_workspace_brings_its_members_and_their_path_dependencies() {
    let repo = Repo::released();
    repo.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nversion = \"0.2.0\"\ndescription = \"A demo that does one thing.\"\nreadme = \"README.md\"\n",
    );
    repo.write(
        "crates/demo/Cargo.toml",
        "[package]\nname = \"demo\"\nversion.workspace = true\ndescription.workspace = true\nreadme.workspace = true\n\n[dependencies]\ndemo-core = { path = \"../demo-core\", version = \"0.1.0\" }\nserde = { version = \"1.0.0\" }\n",
    );
    repo.write(
        "crates/demo-core/Cargo.toml",
        "[package]\nname = \"demo-core\"\nversion.workspace = true\n",
    );
    repo.write(
        "release.toml",
        &RELEASE.replace(
            "manifest = \"Cargo.toml\"",
            "manifest = \"crates/demo/Cargo.toml\"",
        ),
    );
    repo.commit("refactor!: split the crate", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);

    assert_eq!(failing(&plan), ["manifests"], "{:#?}", plan.checks);
    let manifests = find(&plan, "manifests");
    assert!(
        manifests
            .details
            .iter()
            .any(|d| d.contains("crates/demo/Cargo.toml `dependencies.demo-core.version` = 0.1.0")),
        "{manifests:?}"
    );
    assert!(
        !manifests.details.iter().any(|d| d.contains("serde")),
        "a registry dependency is someone else's number: {manifests:?}"
    );

    repo.write(
        "crates/demo/Cargo.toml",
        "[package]\nname = \"demo\"\nversion.workspace = true\ndescription.workspace = true\nreadme.workspace = true\n\n[dependencies]\ndemo-core = { path = \"../demo-core\", version = \"0.2.0\" }\n",
    );
    repo.commit("fix: name the member's version", NOW - DAY + 120);
    let plan = repo.plan(&Held::released(), None);
    assert!(plan.ready, "{:#?}", plan.checks);
    // Breaking before 1.0 is a minor step.
    assert_eq!(
        plan.proposed.as_ref().map(|p| p.version.clone()),
        Some(Version::new(0, 3, 0))
    );
    assert!(
        plan.steps
            .bump
            .iter()
            .any(|e| e.path == "crates/demo/Cargo.toml" && e.at == "dependencies.demo-core.version"),
        "{:#?}",
        plan.steps.bump
    );
}

#[test]
fn every_problem_in_release_toml_is_reported_at_once() {
    let repo = Repo::released();
    repo.write(
        "release.toml",
        &RELEASE
            .replace("changelog = \"CHANGELOG.md\"", "changelog = \"CHANGES.md\"")
            .replace("workflow = \"ci.yml\"", "workflow = \"build.yml\"")
            .replace("wraps = \"demo\"", "wraps = \"nobody\""),
    );
    let error = match Release::open(repo.path()) {
        Err(Error::Config(problems)) => problems,
        other => panic!("expected a config error, got {:?}", other.map(|_| ())),
    };
    assert_eq!(error.len(), 3, "{error:#?}");
}

#[test]
fn an_unknown_key_in_release_toml_is_refused() {
    let repo = Repo::released();
    repo.write("release.toml", &format!("{RELEASE}\n[extra]\nkey = 1\n"));
    assert!(matches!(Release::open(repo.path()), Err(Error::Config(_))));
}

#[test]
fn a_repository_without_release_toml_says_so() {
    let repo = Repo::empty();
    repo.commit("chore: found", NOW);
    match Release::open(repo.path()) {
        Err(Error::NoConfig(path)) => assert_eq!(canonical(&path), canonical(repo.path())),
        other => panic!("expected NoConfig, got {:?}", other.map(|_| ())),
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("canonical")
}

#[test]
fn the_plan_serializes_to_the_documented_shape() {
    let repo = Repo::released();
    let plan = repo.plan(&Held::released(), Some(&Aimed("0.3.0")));
    let json = serde_json::to_value(&plan).expect("serializes");
    assert_eq!(json["project"], "demo");
    assert_eq!(json["released"], "0.2.0");
    assert_eq!(json["proposed"]["version"], "0.3.0");
    assert_eq!(json["proposed"]["source"], "record");
    assert_eq!(json["proposed"]["bump"], "minor");
    assert_eq!(json["stage"]["friday"], "2026-10-02");
    assert_eq!(json["checks"][0]["name"], "number");
    assert_eq!(json["checks"][0]["status"], "pass");
    assert_eq!(json["steps"]["ci"]["provider"], "github");
    assert_eq!(json["steps"]["post"][0]["kind"], "install");
    assert_eq!(json["steps"]["publish"][1]["registry"], "npm");
    assert_eq!(json["ready"], true);
}

#[test]
fn a_wrapper_may_share_its_crates_name() {
    let repo = Repo::released();
    repo.write("npm/package.json", &NPM.replace("demo-cli", "demo"));
    repo.write(
        "release.toml",
        &RELEASE.replace("name = \"demo-cli\"", "name = \"demo\""),
    );
    repo.commit(
        "build: publish the wrapper under the crate's name",
        NOW - DAY + 60,
    );
    let plan = repo.plan(&Held::released(), None);
    assert!(plan.ready, "{:#?}", plan.checks);
    assert_eq!(
        find(&plan, "descriptions").summary,
        "the wrapper says what it wraps says"
    );
}

#[test]
fn an_example_app_may_keep_its_own_readme() {
    let repo = Repo::released();
    repo.write(
        "examples/web/package.json",
        "{ \"name\": \"web-example\" }\n",
    );
    repo.write("examples/web/README.md", "# Run the example\n");
    repo.commit("docs: an example app", NOW - DAY + 60);
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(status(&plan, "readme"), Status::Pass, "{:#?}", plan.checks);
}

#[test]
fn after_one_a_breaking_fix_is_a_major_step() {
    let repo = Repo::released();
    for (path, text) in [
        ("Cargo.toml", CARGO.replace("0.2.0", "1.4.0")),
        ("npm/package.json", NPM.replace("0.2.0", "1.4.0")),
        ("README.md", README.replace("0.2.0", "1.4.0")),
        (
            "CHANGELOG.md",
            "# Changelog\n\n## [1.4.0] - 2026-09-20\n".to_owned(),
        ),
    ] {
        repo.write(path, &text);
    }
    repo.commit("chore(release): v1.4.0", NOW - DAY + 60);
    repo.git(&["tag", "-a", "v1.4.0", "-m", "v1.4.0"], NOW - DAY + 60);
    repo.commit(
        "fix: drop the old flag\n\nBREAKING CHANGE: --old is gone",
        NOW - DAY + 120,
    );
    let held = Held {
        crates: Ok(Some(vec!["1.4.0"])),
        npm: Ok(Some(vec!["1.4.0"])),
        asked: RefCell::new(0),
    };
    let plan = repo.plan(&held, None);
    assert!(plan.ready, "{:#?}", plan.checks);
    assert_eq!(plan.tally.breaking, 1);
    assert_eq!(
        plan.proposed.map(|p| p.version),
        Some(Version::new(2, 0, 0))
    );
}

#[test]
fn a_section_for_the_new_number_is_a_duplicate_until_the_manifests_carry_it() {
    let repo = Repo::released();
    repo.write(
        "CHANGELOG.md",
        "# Changelog

## [0.3.0] - 2026-10-01

## [0.2.0] - 2026-09-01
",
    );
    let plan = repo.plan(&Held::released(), None);
    assert_eq!(failing(&plan), ["changelog"], "{:#?}", plan.checks);

    // The release edits made: the number in the manifests and the texts.
    repo.write("Cargo.toml", &CARGO.replace("0.2.0", "0.3.0"));
    repo.write("npm/package.json", &NPM.replace("0.2.0", "0.3.0"));
    repo.write("README.md", &README.replace("0.2.0", "0.3.0"));
    let plan = repo.plan(&Held::released(), None);
    assert!(plan.ready, "{:#?}", plan.checks);
    assert!(plan.steps.bump.is_empty(), "{:#?}", plan.steps.bump);
}
