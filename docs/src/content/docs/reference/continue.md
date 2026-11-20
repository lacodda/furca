---
title: continue
description: Go on with the merge, cherry-pick, revert or rebase halfway here once nothing is in conflict; with --skip, leave out the commit it stopped at.
---

```sh
furca continue [--skip] [-C PATH] [--json]
```

Goes on with whatever git has halfway here - a merge, a cherry-pick, a revert, a
rebase, `git am` - whether furca began it or a terminal did: furca reads which it is,
so you do not name it. The commit it stopped at is made with the message git wrote
for it - no editor opens - and your hooks run; a pick, a revert or a rebase then goes
on with its next commit, and may stop at another conflict.

Nothing goes on while a path is still in conflict: resolve each with
[`furca resolve`](/furca/reference/resolve/) first. A merge is one step and has
nothing to skip; a bisect goes on as commits are marked good or bad with `git
bisect`, and can only be aborted here.

| Option | Meaning |
| --- | --- |
| `--skip` | Leave out the commit it stopped at - its conflicts with it - and go on with the next. |

```console
$ furca continue
furca: 1 path is still in conflict: resolve it first

$ furca resolve src/client.rs --take both
Resolved src/client.rs, every conflict with both sides, ours first.
Nothing left in conflict: furca continue goes on.

$ furca continue
Done: main is at d816da5.
```

If your `commit-msg` hook refuses git's message, nothing moves: commit the resolution
with a message of your own, with `git commit`, and the merge is finished as well.

## JSON

The same shape as [`merge`](/furca/reference/merge/#json).

## Exit status

`0` when the operation is done; `1` when it stopped at another conflict; `2` when
furca or git refused and nothing moved - nothing halfway, a path still in conflict, a
pick that came out empty.

## Related

- [abort](/furca/reference/abort/) - puts everything back as it was before the operation began.
- [conflicts](/furca/reference/conflicts/) - what is halfway, and what is left.
