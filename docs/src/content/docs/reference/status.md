---
title: status
description: Show where HEAD is, and the working copy against it - what is staged, what is not, what is untracked.
---

```sh
furca status [-C PATH] [--json]
```

Opens the repository containing `PATH` (the current directory if omitted),
walking up through parent directories the way `git` does, and says where
`HEAD` is, then what differs from it:

```console
$ furca status
On main at 171154d, tracking origin/main

Staged:
  M src/lib.rs       +3 -1
  R old.rs -> new.rs  +0 -0
  A assets/logo.png  binary, 24.0 KB

Not staged:
  M src/lib.rs  +1 -0
  D gone.txt    text, 8 B

Untracked:
  A notes.md  +12 -0
```

A clean working tree is the first line alone. A repository without commits
reads `On main, no commits yet`; a detached `HEAD` reads `HEAD detached at
3f1c2b9`.

## The lists

| List | What it holds |
| --- | --- |
| **Conflicted** | Paths a merge left unresolved, and how the two sides disagree. |
| **Staged** | What the next commit would change: `HEAD` against the index. A file moved with `git mv` is one renamed row. |
| **Not staged** | What the index does not have: the index against the working tree. A file moved by hand is a deletion here and an untracked file below until it is staged, as in `git status`. |
| **Untracked** | Files git does not track and does not ignore, each on its own, as `git status --untracked-files=all` lists them. Empty when `status.showUntrackedFiles` is `no`. |

The lists are what `git status --porcelain=v2` says, file for file. A file on
disk is compared as `git add` would take it, through line-ending conversion and
clean filters, git LFS's among them: with `core.autocrlf` on, a checkout's
`\r\n` is no change.

Each row says how many lines moved, or for a file with no lines what it is and
how large: a binary, a git LFS file by the size of the file rather than the
pointer, a submodule. [show](/furca/reference/show/) explains how a file is
told apart.

Nothing is written. `git status` refreshes the timestamps in the index as it
goes; this does not, and changes nothing in the repository.

[diff](/furca/reference/diff/) shows the lines of either list.

## JSON

```console
$ furca status --json
{
  "branch": "main",
  "commit": "171154d1429f569e4ac61e6587e610f36568af55",
  "detached": false,
  "upstream": "origin/main",
  "staged": [],
  "unstaged": [
    {
      "path": "src/lib.rs",
      "status": "modified",
      "from": null,
      "similarity": null,
      "before": { "id": "a99ce086…", "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 110 },
      "after": { "id": null, "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 126 },
      "lines": { "added": 2, "removed": 2 }
    }
  ],
  "untracked": [],
  "conflicted": []
}
```

| Field | Meaning |
| --- | --- |
| `branch` | The branch `HEAD` is on; `null` when detached. |
| `commit` | The full id `HEAD` resolves to; `null` before the first commit. |
| `detached` | `true` when `HEAD` points at a commit rather than a branch. |
| `upstream` | The remote-tracking branch the current branch follows, such as `origin/main`; `null` if none is configured. |
| `staged`, `unstaged`, `untracked` | The files of each list, as [`show --json`](/furca/reference/show/#json) describes a file. An `untracked` file is added, with nothing before it. |
| `*.after.id` | `null` for a file on disk, which git has stored under no id yet. |
| `*.before.content`, `*.after.content` | As in `show`, or `{ "kind": "unread" }` for a file that could not be read: gone since it was listed, or a filter that failed. [diff](/furca/reference/diff/) on it says why. |
| `conflicted[].path` | A path a merge left unresolved. |
| `conflicted[].kind` | `both-modified`, `both-added`, `both-deleted`, `added-by-us`, `added-by-them`, `deleted-by-us` or `deleted-by-them`. |

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [diff](/furca/reference/diff/) - the lines of what is staged, or of what is not.
- [refs](/furca/reference/refs/) - every branch and tag, not only the current one.
- [line](/furca/reference/line/) - where many repositories stand at once.
- [The window](/furca/guides/the-window/#local-changes) - the same lists, kept up to date as files are saved.
- [The engine](/furca/guides/the-engine/) - how the answer is computed.
