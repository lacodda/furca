# 0006 — The release engine is a crate of its own

Date: 2026-10-04
Status: Accepted. Moves the engine of ADR 0003 out of `furca-core`; the pipeline and invariants of ADR 0003 stand.

## Context

ADR 0003 put the release pipeline inside `furca-core` and named its own cost: a second responsibility for one crate. Two things have made that cost real.

- `furca-core` is becoming the git library of the line: rigger reads its history through it, scheda will read file history through it. Those callers want refs and commits, and should not compile TOML parsing, an HTTP client and a registry protocol to get them.
- The release engine has callers of its own besides the `furca` CLI: the desktop panel later, and lyrn's release checks, which call the engine instead of keeping a copy.

Planning a release also needs things a git library has no business owning: the shape of `release.toml`, reading Cargo and npm manifests, asking crates.io and npm which numbers are taken, asking the owner's record (rigger) which stage is being built.

## Decision

The engine is `furca-release`, a crate in this workspace, published next to `furca-core` and `furca`:

- It reads git only through `furca-core` (history since a tag, whole messages, refs, tracked paths) and adds no git access of its own. Mutations, when the run arrives, go through the same `git_cmd` boundary as everything else (ADR 0001).
- `release.toml` at the repository root is its whole configuration: the manifests that carry the version, the changelog, the gate commands, the CI provider and workflows, the packages per registry, the steps after the tag. It is public and holds nothing private; whatever belongs to the owner's machine (which stand a deploy goes to, where the record lives) is asked of the tool that keeps it.
- `plan` is read-only. It proposes the number - from the record's stage when there is one, else from the commits, else from the manifests for a first release - and judges it against the tags and the registries: the next free number, never a hole, never less than the commits ask for. It runs the consistency checks every repository of the line used to keep its own copy of.
- Commits are read with `git-conventional`, the parser `git-cliff-core` uses, so the plan's reading of a commit and the changelog's are one reading.

## Consequences

**Positive.** `furca-core` stays a library about repositories. The consistency checks exist once: a repository gets them by having a `release.toml`, and furca's own test calls the engine on furca rather than restating the rules. The record and the registries are behind traits, so the plan is tested on temporary repositories with no network and no rigger.

**Negative.** Three crates to publish in order (`furca-core`, `furca-release`, `furca`), and a new crate is published by hand the first time, before trusted publishing can be set up for it. The CLI depends on an HTTP client and TLS stack it did not need before.
