---
title: The engine
description: How furca splits reads and mutations between gitoxide and the system git.
---

Furca's engine, `furca-core`, reads a repository in-process through
[gitoxide](https://github.com/GitoxideLabs/gitoxide) and performs mutations
through the system `git` CLI. See
[ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md)
for why the split falls where it does.

The same engine is exposed through three doors — the desktop app, the `furca`
CLI, and eventually an MCP server — each a thin wrapper with no logic of its
own. See
[ADR 0002](https://github.com/lacodda/furca/blob/main/docs/adr/0002-one-core-three-doors.md).

## The release engine

Releasing is a crate of its own, `furca-release`, on top of `furca-core`: it
reads a repository's `release.toml`, proposes the next version, checks
everything that has to agree before a tag, and then carries the release out -
bump, changelog, gate, commit, push, CI, tag, publish, then the shop
windows, the install and the deploy, each proven, and the record - see
[release](/furca/reference/release/). It reads git through `furca-core` and
changes it through the same git door as everything else, so the split above
holds for it too. See
[ADR 0006](https://github.com/lacodda/furca/blob/main/docs/adr/0006-the-release-engine-is-a-crate-of-its-own.md)
and, for why a run is redone before its commit and resumed after it,
[ADR 0007](https://github.com/lacodda/furca/blob/main/docs/adr/0007-a-release-run-is-redone-before-its-commit-and-resumed-after.md);
for why the steps after the tag prove what they did,
[ADR 0008](https://github.com/lacodda/furca/blob/main/docs/adr/0008-after-the-tag-the-run-proves-what-went-out.md).

## Mutations

Everything that changes a repository or talks to a remote - a commit, a tag,
a push, asking a remote for a reference - runs the system `git` through one
door, `Repository::git()`. Each run goes into the repository's command log,
which is what a release report lists and what the window's console will show.
The door runs `git` without a terminal to answer prompts: a credential that
is missing fails at once instead of waiting for input nobody gives.
