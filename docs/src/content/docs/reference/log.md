---
title: log
description: Show the history, newest first, parents after their children.
---

```sh
furca log [-C PATH] [-n LIMIT] [--all] [--json]
```

Walks the history from `HEAD` and prints one line per commit: short id, author
date, author, subject.

```console
$ furca log -n 3
171154d 2026-09-17 Ada Author  docs: fill the readme out as a shopfront
d402e85 2026-09-17 Ada Author  chore: point components.json at the dowel registry
c8292c5 2026-09-04 Ada Author  build: raise the MSRV to 1.88
... more history; raise --limit (now 3)
```

## Order

A parent is never listed before any of its children, so the list can be drawn
as a graph from top to bottom. Within that rule, newer commits come first -
the order of `git log --date-order`.

Commit times alone cannot give that guarantee: a machine with a slow clock
records a child as older than its parent, and sorting by time would put the
parent first. furca orders by the graph and uses time only to break ties.

## JSON

```console
$ furca log -n 1 --json
{
  "commits": [
    {
      "id": "171154d1429f569e4ac61e6587e610f36568af55",
      "parents": ["d402e85062da326a349e4d8f2dc3f36ebf7955d2"],
      "author": {
        "name": "Ada Author",
        "email": "ada@example.com",
        "time": "2026-09-17T17:06:38-03:00"
      },
      "committer": {
        "name": "Ada Author",
        "email": "ada@example.com",
        "time": "2026-09-17T17:06:38-03:00"
      },
      "subject": "docs: fill the readme out as a shopfront"
    }
  ],
  "truncated": true
}
```

| Field | Meaning |
| --- | --- |
| `commits[].parents` | Parent ids in order; the first is the mainline, a merge has two or more. |
| `commits[].author.time` | RFC 3339 with the person's own UTC offset, not converted to yours. |
| `commits[].subject` | The first paragraph of the message, folded to one line. |
| `truncated` | `true` when the walk stopped at the limit with more history behind it. |

A repository without commits prints an empty `commits` list, not an error.

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `-n, --limit <N>` | How many commits to show at most. Defaults to `100`. |
| `--all` | Start from every branch, remote-tracking branch and tag as well as `HEAD`, as `git log --all` does - the history a graph shows. |
| `--json` | Print JSON instead of text. |
