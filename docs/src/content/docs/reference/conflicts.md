---
title: conflicts
description: List what is in conflict and what is halfway, or show one file's conflicting blocks with both sides and their common ancestor.
---

```sh
furca conflicts [PATH] [-C PATH] [--json]
```

Without a path, says what git has halfway here - and what it is bringing in - and
lists every path in conflict with how the two sides disagree. With one, shows that
file's conflicts: each block where both sides changed the same lines differently,
with our lines, the common ancestor's and theirs, as diff3 writes them.

The blocks are read from the three versions git keeps of a conflicted file - the
common ancestor, ours and theirs - and not from the markers in the working tree,
so they are the same whatever `merge.conflictStyle` says, and whatever has been
done to the file since. A change only one side made, or both made alike, is no
conflict: it goes in on its own.

```console
$ furca conflicts
A merge is halfway on main: Merge branch 'feature/retry'.
  both modified    src/client.rs
  deleted by them  src/legacy.rs

$ furca conflicts src/client.rs
src/client.rs: both modified
The working tree's file holds git's markers from line 4.

conflict 1 of 2: line 4 of ours, line 4 of theirs
<<<<<<< ours
pub const TIMEOUT: Duration = Duration::from_secs(15);
||||||| base
pub const TIMEOUT: Duration = Duration::from_secs(10);
=======
pub const TIMEOUT: Duration = Duration::from_secs(30);
>>>>>>> theirs
…
```

A file one side deleted, or one that is not text, has no blocks: it is taken whole,
with `furca resolve PATH --ours`, `--theirs` or `--delete`.

## JSON

Without a path:

```console
$ furca conflicts --json
{
  "halfway": {
    "operation": "merge",
    "branch": "main",
    "commit": { "id": "1d24b76…", "subject": "refactor: drop the legacy join", … },
    "message": "Merge branch 'feature/retry'",
    "conflicts": 2,
    "can_continue": true,
    "can_skip": false
  },
  "conflicts": [
    { "path": "src/client.rs", "kind": "both-modified" },
    { "path": "src/legacy.rs", "kind": "deleted-by-them" }
  ]
}
```

With a path, the conflict whole:

| Field | Meaning |
| --- | --- |
| `kind` | How the two sides disagree: `both-modified`, `both-added`, `both-deleted`, `added-by-us`, `added-by-them`, `deleted-by-us`, `deleted-by-them`. |
| `base`, `ours`, `theirs` | Each side's file - its blob, mode and what it holds - or `null` for a side with no file. |
| `texts` | The three texts whole, when every side is text: `base`, `ours`, `theirs`. |
| `blocks` | The lines in order, each block with its `kind` - `unchanged`, `ours`, `theirs`, `same` or `conflict` - and its lines in each text as `{ start, count }`, from 0. Empty for a file taken whole. |
| `marked` | The line, from 1, where the working tree's file still holds git's markers; `null` when it holds none. |

## Exit status

`0` when nothing is in conflict; `1` when something is; `2` when the path is not in
conflict, or the repository cannot be read.

## Related

- [resolve](/furca/reference/resolve/) - settles a file and stages it.
- [status](/furca/reference/status/) - the working copy, conflicts among it.
