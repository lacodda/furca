---
title: merge
description: Merge a branch, a remote branch, a tag or a commit into the current branch; a conflict stops it halfway.
---

```sh
furca merge NAME [--no-ff | --ff-only] [-C PATH] [--json]
```

Merges `NAME` into the branch `HEAD` is on, through your own `git`: the message is
the one git writes - `Merge branch 'feature/retry'` - no editor opens, and your hooks
and signing apply as in a terminal. Where the branch only has to move along to what
it merges, it does, as `merge.ff` says; the flags are git's own:

| Flag | git's flag | What it does |
| --- | --- | --- |
| `--no-ff` | `--no-ff` | Makes a merge commit even where moving along would do. |
| `--ff-only` | `--ff-only` | Only moves along, and refuses when the two have parted. |

What `NAME` means is decided before git is asked: a local branch of that name, then a
remote-tracking branch such as `origin/main`, then a tag - as a checkout looks for one
- or a full name such as `refs/tags/v1.0.0`, or else any commit. git is told the
short name when it would find the same ref by it, so the message names it as you do;
the full name when a tag or a branch of the same name would stand in for it. The
branch `HEAD` is on is refused, and so is a merge while another operation is halfway.

```console
$ furca merge topic/docs
Merged 1 commit: main is at 3478b58.

$ furca merge feature/retry
A merge stopped halfway, 2 paths in conflict:
  both modified    src/client.rs
  deleted by them  src/legacy.rs
Resolve them with furca resolve, then furca continue - or furca abort.
```

A merge that meets a conflict has not failed: it is halfway, as git leaves it.
[`furca conflicts`](/furca/reference/conflicts/) shows what is left,
[`furca resolve`](/furca/reference/resolve/) settles each file,
[`furca continue`](/furca/reference/continue/) makes the merge commit, and
[`furca abort`](/furca/reference/abort/) puts everything back.

## JSON

```console
$ furca merge feature/retry --json
{
  "operation": "merge",
  "stopped": "Auto-merging src/client.rs\nCONFLICT (content): Merge conflict in src/client.rs\n…",
  "conflicts": [
    { "path": "src/client.rs", "kind": "both-modified" },
    { "path": "src/legacy.rs", "kind": "deleted-by-them" }
  ],
  "branch": "main",
  "from": "609c33c…",
  "to": "609c33c…",
  "made": 0
}
```

| Field | Meaning |
| --- | --- |
| `operation` | The operation still halfway - `merge`, `cherry-pick`, `revert`, `rebase` - or `null` once it is done. |
| `stopped` | What git said when it stopped halfway; `null` when it did not stop. |
| `conflicts` | The paths left to resolve, each with how the two sides disagree. |
| `branch` | The branch `HEAD` is on; `null` on no branch. |
| `from`, `to` | The commit `HEAD` was on before, and is on now. |
| `made` | Commits `HEAD` has now that it did not before: what the merge brought in, its merge commit with them. |

[`cherry-pick`](/furca/reference/cherry-pick/), [`revert`](/furca/reference/revert/),
`continue` and `abort` answer in the same shape.

## Exit status

`0` when the merge is done; `1` when it stopped halfway; `2` when furca or git
refused and nothing moved.

## Related

- [conflicts](/furca/reference/conflicts/) - what is left to resolve, and one file's conflicting blocks.
- [cherry-pick](/furca/reference/cherry-pick/), [revert](/furca/reference/revert/) - one commit's change, onto the current branch or taken back.
