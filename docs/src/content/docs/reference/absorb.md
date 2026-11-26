---
title: absorb
description: Put what is staged into a commit already made, as if it had been made with it, and replay the commits after it - or change nothing.
---

```sh
furca absorb REVISION [-C PATH] [--json]
```

Puts what is staged into the commit `REVISION`, as if it had been made with it, and
replays the commits after it on the result: the fixup commit and `git rebase
--autosquash` a person otherwise types, in one step. Stage what belongs there first -
[`furca status`](/furca/reference/status/) says what is - and what is not staged stays
in the working tree.

```console
$ furca status
On main at b15cab2, tracking origin/main

Staged:
  M src/log.rs  +1 -1

Not staged:
  M src/retry.rs  +1 -0

$ furca absorb 1058da2
Absorbed, and 2 commits rewritten: main is at 261de63.
```

An absorb happens whole or not at all. If the change does not fit that commit, or a
commit after it does not replay over it, the branch, the index and the working tree
are put back exactly as they were, and furca says which files it stopped at:

```console
$ furca absorb 1058da2
furca: 1058da2 does not take this change: replaying it there stopped at a conflict in src/retry.rs - that commit, or one after it, changes the same lines. Nothing was changed.
```

`REVISION` is a commit of the line `HEAD` came by, with no merge from it on. A commit
the branch's upstream has already is rewritten like any other: the next push has to
be [`--force-with-lease`](/furca/reference/push/). The window absorbs a single hunk,
or the lines you pick, and leaves everything else staged as it was: see
[the window](/furca/guides/the-window/#rebasing-squashing-and-absorbing).

## JSON

The same shape as [`merge`](/furca/reference/merge/#json); `made` is how many commits
were rewritten - the one that took the change, and each one after it.

## Exit status

`0` when the change is in the commit; `2` when furca or git refused and nothing
moved.
