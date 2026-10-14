# Changelog

All notable changes to this project are documented in this file.

## [0.5.0] - 2026-10-14

### Bug Fixes
- Wait for the tag's other workflows once the registries hold the version

### Build
- Let a running furca be replaced and leave the PATH alone

### Documentation
- Document verify, the install proof and the tag trigger

### Features
- Verify, install and deploy what a release put out
- Put a held publish tag trigger back in the release commit
- Refuse installers that ask for a target never built

### Breaking Changes
- `[[post]] kind = "install"` in release.toml is now
`[post.install]` (with `bin` and `aliases`) and `[post.deploy]`; the
report loses `after`, and `Runner` asks for a `web` and a `machine`.

## [0.4.0] - 2026-10-11

### Build
- Install the CLI by the scripts only and stop publishing its crate

### CI
- Install the CLI in a workflow with the repository's own action

### Documentation
- Document running a release, push and checks

### Features
- Read a commit by any revision that names it
- Change a repository through git and read its upstream
- Run a release from the bump to the record, and resume it
- Read the address of a remote
- Run and resume a release, push and wait for CI, show checks

### Testing
- Format the read-a-commit test

### Breaking Changes
- `Error` is `#[non_exhaustive]` and has new variants for a failed or missing git; a match on it needs a wildcard arm.
- `Record` asks implementors for `shipped`, and `Error` is `#[non_exhaustive]` with new variants for the run.
- The furca crate gets no new versions on crates.io; install the CLI with tools/install.ps1 or tools/install.sh, or build it with `cargo install --git https://github.com/lacodda/furca furca`.

## [0.3.0] - 2026-10-05

### Bug Fixes
- Take the new changelog section as the release's own once bumped

### CI
- Publish the release engine and skip what crates.io holds
- Give jq its filter in the msrv job

### Documentation
- Write the example tag as vX.Y.Z

### Features
- Read the history since a revision, whole messages and the index
- Plan the next release from release.toml
- Plan a release and tell a no from an error

### Testing
- Hold furca to the release engine's checks

### Breaking Changes
- An error now exits 2 instead of 1, so a script can tell "could not answer" from the answer "no".

## [0.2.0] - 2026-09-26

### Performance
- Walk the graph lazily over generation numbers

### Testing
- Gate the speed budgets with a benchmark

### Documentation
- Publish the budgets and what each release measured
- Build the budgets page locally without the releases API

### Build
- Update the toolchain and dependencies

### CI
- Publish both crates on a tag again

### Breaking Changes
- `furca-core`'s `Error::Open` carries a boxed source instead of `gix::discover::Error`, which gix 0.88 removed.

## [0.1.0] - 2026-09-23

### Build
- Raise the MSRV to 1.88

### CI
- Ship the CLI archives only, with installers that unpack them

### Documentation
- Fill the readme out as a shopfront
- Document status, refs and log and install from the release

### Features
- Read refs and walk the history in topological date order
- Add refs and log, text by default and --json on request

### Testing
- Gate the docs address, the command pages and the installers
