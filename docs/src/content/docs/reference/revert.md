---
title: revert
description: Undo the change commits made, each with a new commit that says so.
---

```sh
furca revert REVISION... [-C PATH] [--json]
```

Undoes the change each commit made, in the order given, with a new commit on the
branch `HEAD` is on - `Revert "docs: mention retries"`, the message git writes -
through your own `git`. Nothing is rewritten: the commit stays in the history, and
so does its undoing. A merge commit is undone against its first parent: what it
brought into the line it was merged into goes.

```console
$ furca revert HEAD
Reverted with 1 commit: main is at 7fc3c87.
```

A revert that meets a conflict stops halfway, as a [merge](/furca/reference/merge/)
does: resolve the files, then [`furca continue`](/furca/reference/continue/), `furca
continue --skip` or [`furca abort`](/furca/reference/abort/). Another operation
halfway is refused.

## JSON

The same shape as [`merge`](/furca/reference/merge/#json); `made` is how many commits
the revert made.

## Exit status

`0` when every commit was reverted; `1` when it stopped halfway; `2` when furca or
git refused and nothing moved.
