---
title: tag
description: List the tags, or make, delete or push one.
---

```sh
furca tag [-C PATH] [--json]
furca tag create NAME [TARGET] [-m MESSAGE]
furca tag delete NAME [--remote NAME | --all-remotes]
furca tag push NAME [--remote NAME | --all-remotes]
```

Each runs your own `git`. Without a verb it lists the tags by name, with the
commit each one tags:

```console
$ furca tag
f1380f3 reviewed
7012074 v1.0.0 (annotated)
```

| Verb | What it does |
| --- | --- |
| `create` | Tags `TARGET` - a commit, a branch, another tag; `HEAD` when left out. With `-m` it is an annotated tag; without, a plain one. |
| `delete` | Deletes a tag here; with `--remote NAME` or `--all-remotes`, takes it away from there too. |
| `push` | Pushes a tag where the current branch is pushed, to `--remote NAME`, or to every remote with `--all-remotes`. |

## Annotated and plain

An **annotated** tag (`-m`) is an object of its own: its message, who made it
and when, and a signature when `tag.gpgSign` says so. It is what a release is:
a [push](/furca/reference/push/) of its branch takes it along.

A **plain** tag is a bookmark: a name on a commit and nothing more, never
signed. It stays on this machine until it is pushed by name.

```console
$ furca tag create v1.0.0 -m "First release"
Tagged 7012074 v1.0.0 (annotated).

$ furca tag create reviewed HEAD~1
Tagged f1380f3 reviewed.
```

## Pushing and deleting

`push` goes where a push of the current branch goes - its `pushRemote`,
`remote.pushDefault`, the remote it follows - and with `HEAD` on no branch to
`remote.pushDefault`; with none of those, to the only remote, or `origin`. A
remote that already has a tag of that name somewhere else is not changed: git
refuses. Each remote is said apart, and one that refuses does not stop the
rest.

```console
$ furca tag push v1.0.0
Pushed v1.0.0 to origin

$ furca tag delete v1.0.0 --remote origin
Deleted v1.0.0, which was at 7012074.
Taken away from origin.
```

A tag deleted only here comes back with the next
[fetch](/furca/reference/fetch/) from a remote that still has it: take it away
from there too. A tag only a remote has is taken away from there, with nothing
to delete here.

## JSON

`create` prints the tag as [`furca refs --json`](/furca/reference/refs/#json)
gives each of its `tags`; without a verb, the list of them. `push` and `delete`
say what each remote did:

```console
$ furca tag push reviewed --json
{
  "tag": "reviewed",
  "pushed": [{ "remote": "origin", "tag": "reviewed", "deleted": false }],
  "failed": []
}

$ furca tag delete reviewed --all-remotes --json
{
  "deleted": { "name": "reviewed", "target": "f1380f3…", "lost": 0 },
  "remotes": [{ "remote": "origin", "tag": "reviewed", "deleted": true }],
  "failed": []
}
```

| Field | Meaning |
| --- | --- |
| `pushed[]`, `remotes[]` | Each remote the tag reached or was taken away from, in order. |
| `failed[]` | Each remote that refused: `remote`, `to` (the tag's full name) and `error`, in git's words. |
| `deleted` | The tag as it was here, with the commit it was at; `null` when only the remotes had it. |

## Exit status

`0` when every remote took it; `2` when one refused, or git refused here.

## Related

- [branch](/furca/reference/branch/) - the other names in the history.
- [release](/furca/reference/release/) - makes the release tag itself, annotated, and pushes it once CI is green.
