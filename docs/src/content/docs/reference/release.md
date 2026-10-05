---
title: release
description: Plan the next release from release.toml - the number, what stands in its way, and what a run would do.
---

```sh
furca release plan [-C PATH] [--json]
```

Reads `release.toml` at the root of the repository and says what the next
release is, what stands in its way and what a run would do. It changes
nothing: no file, no tag, no commit. Running the plan comes in a later
release; until then the plan is the checklist.

```console
$ furca release plan
furca 0.2.0 -> 0.3.0, a minor step, from the record
stage v0.3.0 "Release engine: plan"

  pass  number        v0.3.0 is free: a minor step after v0.2.0
                        the commits alone would give v0.2.1
  pass  commits       1 commit since v0.2.0: 1 other
  pass  stack         updated today
                        297fbea chore(deps): update the toolchain and dependencies on 2026-10-04
  pass  manifests     7 fields say 0.2.0, the bump rewrites them
  pass  readme        one README, its links absolute
  pass  descriptions  3 packages describe themselves
  pass  texts         1 follows the bump, 3 name a later version
                        later: crates/furca-core/benches/budgets.json:25: v0.7.0
  pass  changelog     the new section goes under the header of CHANGELOG.md
  warn  registries    furca-release is not published yet: the first publish is by hand
                        crates.io `furca-core`: 0.2.0
                        crates.io `furca-release` is not published yet: the first publish is by hand, and only then can a tag publish it
                        crates.io `furca`: 0.2.0
  pass  record        stage v0.3.0 "Release engine: plan", not aimed at a week yet

The run would
  bump       Cargo.toml workspace.package.version: 0.2.0 -> 0.3.0
             crates/furca-cli/Cargo.toml dependencies.furca-core.version: 0.2.0 -> 0.3.0
             package.json version: 0.2.0 -> 0.3.0
             README.md line 50: v0.2.0 -> v0.3.0
  changelog  a section for 0.3.0 in CHANGELOG.md
  gate       cargo fmt --all --check
             cargo test --workspace
  commit     chore(release): v0.3.0
  ci         wait for ci.yml on GitHub
  tag        v0.3.0, which starts publish.yml
  publish    crates.io furca-core (0.2.0 now)
             crates.io furca-release (not there yet: the first publish is by hand)
             crates.io furca (0.2.0 now)
  after      install

Ready: nothing stands in the way.
```

Lists are shortened here; the command prints every line.

## The number

The plan proposes one number, from the first source that has one:

1. **The record.** When [rigger](https://lacodda.github.io/rigger/) is
   installed and keeps this project at this path, the stage it says is being
   built names the version - `v0.3.0`, or `v1.15` for 1.15.0. The record is
   only asked; furca never writes to it.
2. **The commits** since the last release tag, read as
   [Conventional Commits](https://www.conventionalcommits.org/): a breaking
   change (`!` or a `BREAKING CHANGE:` footer) is a major step, a `feat` a
   minor one, anything else a patch. Before 1.0 a breaking change is a minor
   step.
3. **The manifests**, for a first release with no tag and nothing in any
   registry yet.

Whatever the source, the number has to be the next free one. Taken means a
tag or any version a registry has ever held - crates.io keeps yanked versions
and npm never reuses an unpublished one. A number that skips one (0.7.0 after
0.5.0) is refused, and so is a number smaller than the commits ask for (a
patch over a feature). A stage that names another number is renamed, not
followed: the release takes the next free number.

## Checks

| Check | Fails when |
| --- | --- |
| `number` | The number is taken, skips one, or is too small for the commits. |
| `commits` | Nothing has been committed since the last release. Commits outside the convention are a warning. |
| `stack` | The last `chore(deps)` commit is seven days old or more, or there is none: the stack is updated before a new version, at most once a week. |
| `manifests` | The files in `manifests` disagree, or say neither the last release nor the next. A Cargo workspace brings in its members and the `version` of every path dependency between them. A package's manifest must be among the files the bump rewrites, name the package as `release.toml` does and allow publishing. |
| `readme` | There is no `README.md` at the root, a published package's directory keeps a README of its own, a crate's `readme` is not the root one, or a link in it is relative (it breaks on crates.io and npm). |
| `descriptions` | A package has no description, or a wrapper describes the product in other words than the package it wraps. |
| `texts` | A text names a stale version of the product - see below. |
| `changelog` | The changelog already has a section for the new number while the manifests still carry the old one - the run would write it twice. Once the manifests carry the new number, the section is the release's own. A missing section for the last release is a warning. |
| `registries` | A registry could not be asked, so the number is not proven free. A package no registry has seen is a warning: the first publish is by hand, and only then can a tag publish it. |
| `record` | Never fails. A warning when there is no stage to read, or when the record runs another gate than `release.toml`. |

`warn` is worth reading and does not stop the release; `fail` does.

## Versions in texts

Every tracked file that carries samples and examples - Markdown, workflows,
installers, JSON, TOML, HTML, scripts - is read for versions of the product:
`v1.2.3`, or a version after one of its names (`furca 1.2.3`,
`furca@1.2.3`). A bare `1.2.3` is left alone; in prose it is as often a
dependency's number. Lockfiles and source code are not read.

Each one is:

- **the last release** - the bump rewrites it to the new number, and the plan
  lists it under `bump`;
- **the new number** - already written;
- **a later version** - a plan for the future, listed and allowed;
- **history** - inside a block that says so;
- **stale** - anything else, and the release stops.

An example that means "some version" writes the shape, `vX.Y.Z`, which is no
version at all and never goes stale. A number that is history - the release
that first did a thing - goes inside a block whose end is stated:

```md
<!-- historical versions -->
| Since | Command |
| --- | --- |
| v0.1.0 | `status` |
<!-- /historical versions -->
```

In files that are not markup the same words go in a `#` or `//` comment. A
block left open is reported, so an exemption cannot quietly cover the rest of
the file. Whole files whose numbers stay as written - ADRs, pages of examples
- are listed in `as_written`.

## release.toml

```toml
[project]
name = "turnout"
changelog = "CHANGELOG.md"
manifests = ["Cargo.toml", "npm/package.json"]
as_written = ["docs/adr/"]

[gate]
commands = [
    "cargo fmt --all --check",
    "cargo clippy --all-targets -- -D warnings",
    "cargo test",
]

[ci]
provider = "github"
workflow = "ci.yml"
publish = "publish.yml"

[[package]]
registry = "crates"
name = "turnout"
manifest = "Cargo.toml"

[[package]]
registry = "npm"
name = "turnout-cli"
manifest = "npm/package.json"
wraps = "turnout"

[[post]]
kind = "install"
```

Paths are relative to the root and use `/`. Every problem in the file is
reported at once; an unknown key is a problem, not something to ignore.

| Key | Meaning |
| --- | --- |
| `project.name` | The product's name: the word its own output prints before a version, and the name the record knows it by. |
| `project.changelog` | The changelog the release adds a section to, under its header. |
| `project.manifests` | Every file that carries the version: a `Cargo.toml` (a package, or a workspace root that brings in its members) or a JSON file with a top-level `"version"`. |
| `project.as_written` | Files, or directories ending in `/`, whose versions are left as written. The changelog always is. |
| `gate.commands` | Shell commands run from the root, in order; the first to fail stops the release before anything is committed. |
| `ci.provider` | `github` or `forgejo`. |
| `ci.workflow` | The workflow that has to be green on the release commit before it is tagged, as a file in `.github/workflows/` or `.forgejo/workflows/`. |
| `ci.publish` | The workflow a tag starts to publish the packages, if there is one. |
| `package[].registry` | `crates` or `npm`. |
| `package[].name` | The name the registry knows the package by; it has to match the manifest. |
| `package[].manifest` | The manifest that describes the package. |
| `package[].wraps` | The package of another registry this one ships the binary of - an npm wrapper around a crate's CLI, which may share its name. The two must describe the product the same way. |
| `post[].kind` | What happens after the tag, in order: `install` installs the released build with the repository's own installer, `deploy` deploys with turnout, whose own configuration names the stand. |

The file is public and holds nothing private - no machine paths, no stand
names, no tokens. What belongs to the owner's machine is asked of the tool
that keeps it.

## JSON

```console
$ furca release plan --json
{
  "project": "furca",
  "released": "0.2.0",
  "proposed": { "version": "0.3.0", "tag": "v0.3.0", "source": "record", "bump": "minor" },
  "stage": { "version": "0.3.0", "title": "Release engine: plan", "week": null, "friday": null },
  "changes": [
    { "id": "297fbea…", "subject": "chore(deps): update the toolchain and dependencies", "kind": "chore", "scope": "deps", "breaking": false }
  ],
  "tally": { "total": 1, "breaking": 0, "features": 0, "fixes": 0, "other": 1, "unconventional": 0 },
  "checks": [
    { "name": "number", "status": "pass", "summary": "v0.3.0 is free: a minor step after v0.2.0", "details": ["the commits alone would give v0.2.1"] }
  ],
  "steps": {
    "bump": [
      { "path": "Cargo.toml", "at": "workspace.package.version", "from": "0.2.0", "to": "0.3.0" },
      { "path": "README.md", "at": "line 50", "from": "v0.2.0", "to": "v0.3.0" }
    ],
    "changelog": "CHANGELOG.md",
    "gate": ["cargo fmt --all --check"],
    "commit": "chore(release): v0.3.0",
    "ci": { "provider": "github", "workflow": "ci.yml", "publish": "publish.yml" },
    "tag": "v0.3.0",
    "publish": [
      { "registry": "crates", "name": "furca-release", "latest": null, "first": true }
    ],
    "post": [{ "kind": "install" }]
  },
  "ready": true
}
```

Lists are shortened here to one entry each.

| Field | Meaning |
| --- | --- |
| `released` | The last version released, by tags and registries; `null` before the first release. |
| `proposed.source` | `record`, `commits` or `manifests` - where the number came from. |
| `proposed.bump` | `patch`, `minor` or `major` from `released`; `null` for a first release. |
| `stage` | The stage the record says is being built; `null` without a record. |
| `changes` | Commits since the last release tag, newest first. `kind` is `null` for a message outside the convention. |
| `checks[].name` | `number`, `commits`, `stack`, `manifests`, `readme`, `descriptions`, `texts`, `changelog`, `registries`, `record` - stable identifiers. |
| `checks[].status` | `pass`, `warn`, `fail`, or `skip` for a source that was not asked. |
| `steps.bump` | Every edit the bump makes: a manifest key, or a line of a text. |
| `steps.publish[].first` | `true` when the registry has never seen the package. |
| `ready` | `true` when no check fails. |

## Exit status

`0` when the release is ready, `1` when something stands in its way, `2` when
there is no plan at all - no `release.toml`, a problem in it, or no
repository.

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [ADR 0006](https://github.com/lacodda/furca/blob/main/docs/adr/0006-the-release-engine-is-a-crate-of-its-own.md) - why the engine is a crate of its own.
- [ADR 0003](https://github.com/lacodda/furca/blob/main/docs/adr/0003-release-engine-inside-the-client.md) - the pipeline and the invariants it holds.
