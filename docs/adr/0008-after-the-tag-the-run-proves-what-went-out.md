# 0008 — After the tag, the run proves what went out

Date: 2026-10-14
Status: Accepted. Extends the run of ADR 0007 past the publish.

## Context

Until v0.5 a run ended at the record and left the rest of the ritual to a person: "install the released build", "deploy with turnout". That rest is where the line kept losing releases:

- **Shop windows went stale unseen.** A release page that lost an archive, a crate that never reached crates.io, a docs site that answered 404 - each was found by someone else, after the fact.
- **A held publish trigger stayed held.** The line's rule for a new public repository is to hold the tag trigger of the publish workflow until the first publish by hand, because a tag cannot create a package. Releasing it again was a step nobody's tool remembered; lyrid shipped nine versions with it held.
- **"Installed" was a claim, not a fact.** kasl's alias kept answering with the old binary after an upgrade; turnout's self-update said it updated and had not. A version printed by `--version` does not prove the file is the release's own build - a local build of the same number prints the same.

## Decision

**The run carries on past the publish, and every step after it proves its result instead of reporting its exit code.** The steps are `verify`, `install` and `deploy`, then the record, which now comes last so it can say what the steps did. A stop in any of them is resumed like any step after the commit.

- **`verify` reads the shop windows** of the tag: the release page is out, not a draft, and carries every file `[page] assets` names; every package's registry holds the version; every link the manifests show - homepage, documentation, repository - answers. The same check is `furca release verify` on its own, for any tag.
- **The tag trigger comes back by itself.** A run never tags before every package is in its registry (ADR 0007), so holding the trigger protects nothing once the engine releases. The plan reads the publish workflow's `on:` with a YAML parser, matches the tag against its filters as GitHub does, and when the trigger is held, the bump puts `push: tags: ['v*']` back in the release commit. A trigger whose filter leaves the tag out, or a shape the engine cannot rewrite safely, fails the plan.
- **`install` runs the repository's own installer** - `tools/install.ps1` on Windows, `tools/install.sh` elsewhere, the line's standard pair - pinned to the tag, then proves the result: the binary the PATH finds prints the version, its SHA-256 equals the binary inside the release page's archive for this machine's target, and every alias the installer makes hashes the same. The archive is downloaded and unpacked by the system `tar` the installers use, so the proof does not trust the installer that produced what it checks.
- **`deploy` runs `turnout deploy`.** Which stand it goes to stays turnout's own configuration; turnout checks the stand's health and rolls a failed deploy back, and its exit status is the step's.
- **The installers' targets are checked before the tag.** Every target triple an installer asks for has to be among `[page] assets`, so an installer cannot ask for an archive the release never builds.

The machine is reached through a `Machine` trait - run a program, find one on the PATH, read its `--version` - so the steps are tested against a scripted machine and run against the real one.

## Consequences

**Positive.** A run that ends green means the release is in every shop window, installed for the owner as the release's own bytes, and on its stand; the record says so. The held trigger, the dead docs link, the alias that lags and the installer that asks for a missing target stop being things to remember.

**Negative.** A run with `[post.install]` downloads the archive for its target a second time, after the installer did. The proof needs the system `tar` (and `unzip` for a zip outside Windows). A first install that adds its directory to the user PATH is not visible to the process that ran it: the step stops and asks for a new terminal and `resume`. Installing into a directory other than the owner's (the install check of `furca release verify --install`) needs installers that honour `{APP}_INSTALL_DIR` and `{APP}_NO_PATH`.
