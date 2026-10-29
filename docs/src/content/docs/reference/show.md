---
title: show
description: Show a commit - who made it, its message, and the files it changed with their lines and sizes.
---

```sh
furca show [REVISION] [-C PATH] [--json]
```

Prints a commit - `HEAD` unless a revision names another: an id, a branch, a
tag - with its whole message and a line for every file it changed: how it
changed, its path, and how many lines it added and removed.

```console
$ furca show
commit f9f9b99d100f6f66145d56952d0c4812761a0d58
Author: Ada Author <ada@example.com>  2026-10-28T14:30:00-03:00

    feat: greet by name

    The greeting now says who it is for. The guide says so too.

 A assets/intro.mp4                git LFS, 120.6 MB
 A assets/logo.png                 binary, 24.0 KB
 R docs/intro.md -> docs/guide.md  +1 -1
 M src/lib.rs                      +2 -2
4 files, +3 -3
```

The letters are git's: `A` added, `D` deleted, `M` modified, `R` renamed, `C`
copied.

## What a file holds

A file that is not text has no lines to count, so its line says what it is
and how large it is instead:

- **binary** - bytes git does not compare by lines: a NUL among the first 8000
  bytes, the `binary` or `-diff` attribute in `.gitattributes`, or a file over
  `core.bigFileThreshold`. A binary committed as it is - not through git LFS -
  is the change a public repository regrets, which is why it is named before
  anyone opens a diff.
- **git LFS** - the pointer git LFS commits in place of a file. The size is the
  file's, not the pointer's few bytes.
- **submodule** - a submodule's commit rather than a file.

## What a commit is compared with

Its first parent. A merge is compared with the line it was merged into, so it
lists what the merge brought in - as Fork shows a merge, and as
`git show --first-parent` does. A root commit is compared with nothing: every
file in it is added.

Renames are found as git finds them, by content, under the repository's own
`diff.renames`. The percentage beside a rename in `--json` is git's
similarity: how much of the file it kept.

## JSON

```console
$ furca show --json
{
  "commit": {
    "id": "f9f9b99d100f6f66145d56952d0c4812761a0d58",
    "parents": ["352b6cafd1455dced507019ae1544cccac688b42"],
    "author": { "name": "Ada Author", "email": "ada@example.com", "time": "2026-10-28T14:30:00-03:00", "seconds": 1793208600 },
    "committer": { "name": "Ada Author", "email": "ada@example.com", "time": "2026-10-28T14:30:00-03:00", "seconds": 1793208600 },
    "subject": "feat: greet by name"
  },
  "message": "feat: greet by name\n\nThe greeting now says who it is for. The guide says so too.",
  "changes": {
    "commit": "f9f9b99d100f6f66145d56952d0c4812761a0d58",
    "parent": "352b6cafd1455dced507019ae1544cccac688b42",
    "files": [
      {
        "path": "assets/intro.mp4",
        "status": "added",
        "from": null,
        "similarity": null,
        "before": null,
        "after": {
          "id": "2ab733ba5aaf7f00a1fdb3753c09e765fb346d4d",
          "mode": "file",
          "content": { "kind": "lfs", "oid": "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393" },
          "size": 126418944
        },
        "lines": null
      },
      {
        "path": "docs/guide.md",
        "status": "renamed",
        "from": "docs/intro.md",
        "similarity": 50,
        "before": { "id": "9d2a26b8…", "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 69 },
        "after": { "id": "39ec9f13…", "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 93 },
        "lines": { "added": 1, "removed": 1 }
      }
    ]
  }
}
```

| Field | Meaning |
| --- | --- |
| `message` | The whole message: subject, body and trailers. |
| `changes.parent` | The parent the commit is compared with; `null` for a root commit. |
| `files[].status` | `added`, `deleted`, `modified`, `renamed` or `copied`. |
| `files[].from` | Where a renamed or copied file came from. |
| `files[].similarity` | How much of a renamed or copied file it kept, in percent, as git counts it. |
| `files[].before`, `files[].after` | The file on each side; `null` where there is none - before an addition, after a deletion. |
| `….mode` | `file`, `executable`, `link` or `submodule`. |
| `….content.kind` | `text`, `binary`, `lfs` or `submodule`. Text also says how its lines end: `lf`, `crlf`, `mixed`, or `no-ending` for a file with no line break at all. |
| `….size` | Bytes, as the file is checked out: for git LFS, the size of the file the pointer stands for. `null` for a submodule. |
| `files[].lines` | Lines added and removed; `null` when a side is not text. |

## Options

| Option | Meaning |
| --- | --- |
| `REVISION` | The commit to show. Defaults to `HEAD`. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |
