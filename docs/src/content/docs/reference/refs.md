---
title: refs
description: List branches, remote-tracking branches and tags.
---

```sh
furca refs [-C PATH] [--json]
```

Lists every named pointer into the history - local branches, remote-tracking
branches and tags - each sorted by name, with the commit it points at:

```console
$ furca refs
* 171154d main -> origin/main
  d402e85 feature/login
  171154d origin/main
  c8292c5 tag: v0.1.0
```

`*` marks the branch `HEAD` is on; `->` names the branch's upstream.

Two things are left out on purpose. Symbolic remote refs such as
`origin/HEAD` only name another remote branch that is already listed. A ref
pointing at an object that is not in the repository - a damaged or shallow
clone - is skipped rather than failing the whole list.

## JSON

```console
$ furca refs --json
{
  "branches": [
    { "name": "main", "target": "171154d…", "upstream": "origin/main", "head": true }
  ],
  "remote_branches": [
    { "name": "origin/main", "remote": "origin", "target": "171154d…" }
  ],
  "tags": [
    { "name": "v0.1.0", "target": "c8292c5…", "annotated": true }
  ]
}
```

Ids are shortened here to fit; the command prints them in full.

| Field | Meaning |
| --- | --- |
| `branches[].upstream` | The remote-tracking branch it follows, or `null`. |
| `branches[].head` | `true` for the branch `HEAD` is on. |
| `remote_branches[].remote` | The configured remote the branch belongs to. Remote names may contain `/`; the longest matching name wins. `null` when no configured remote matches. |
| `tags[].target` | What the tag finally points at: an annotated tag is peeled to its commit. |
| `tags[].annotated` | `true` for a tag object with its own message, `false` for a plain pointer. |

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |
