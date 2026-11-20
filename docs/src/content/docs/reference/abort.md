---
title: abort
description: Abort the merge, cherry-pick, revert or rebase halfway here, and put everything back as it was before it began.
---

```sh
furca abort [-C PATH] [--json]
```

Aborts whatever git has halfway here - `git merge --abort`, `git cherry-pick
--abort`, `git revert --abort`, `git rebase --abort`, `git am --abort`, or `git bisect
reset` - whether furca began it or a terminal did. `HEAD`, the index and the files go
back to where they were before it began, and what was resolved so far is lost.

```console
$ furca abort
Aborted: main is at 7fc3c87, as it was.
```

Nothing halfway is refused, in words that say so.

## JSON

The same shape as [`merge`](/furca/reference/merge/#json); `operation` is `null`.

## Exit status

`0` when it was aborted; `2` when there was nothing to abort, or git refused.

## Related

- [continue](/furca/reference/continue/) - goes on instead, once nothing is in conflict.
