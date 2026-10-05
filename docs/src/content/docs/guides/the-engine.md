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
reads a repository's `release.toml`, proposes the next version and checks
everything that has to agree before a tag - see
[release](/furca/reference/release/). It reads git only through `furca-core`,
so the split above holds for it too. See
[ADR 0006](https://github.com/lacodda/furca/blob/main/docs/adr/0006-the-release-engine-is-a-crate-of-its-own.md).
