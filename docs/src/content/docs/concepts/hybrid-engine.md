---
title: Hybrid engine
description: Why reads and mutations use different paths through git.
---

Furca does not pick one strategy for every git operation. Reads — log,
status, diff, blame, refs — run in-process through gitoxide, because a read
cannot corrupt a repository and its speed is felt on every interaction.
Mutations — commit, merge, rebase, push, pull, stash — run through the system
`git` CLI, because their correctness depends on hooks, credential helpers,
LFS filters and commit signing already configured on the machine.

See [ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md)
for the full reasoning.
