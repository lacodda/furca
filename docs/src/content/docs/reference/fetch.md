---
title: fetch
description: Fetch from the remote the current branch follows, from one remote, or from every one, and say what moved.
---

```sh
furca fetch [REMOTE] [--all] [--prune] [-C PATH] [--json]
```

Fetches the remote's branches and tags, as `git fetch` does, through your own
`git`: its credential helper and ssh-agent answer for it, and furca never asks
for a password. Without a remote it fetches the one the current branch follows,
or `origin`, or the only remote there is; `--all` fetches every remote, as
`git fetch --all` does.

```console
$ furca fetch
Fetched origin: 2 refs moved
  origin/main         4c3ea3f..9b1d2e7
  origin/orbit-math   new at 70262e5
```

What moved is read from the refs before and after, not from what git printed:
a remote-tracking branch that moved, one that is new, one that is gone, and a
new tag. While it runs, how far git has got goes to standard error on one line,
when standard error is a terminal.

`--prune` also takes away the remote-tracking branches whose branch the remote
no longer has; without it, `fetch.prune` in your configuration decides.

## JSON

```console
$ furca fetch --all --json
{
  "remotes": ["mirror", "origin"],
  "moved": [
    { "name": "refs/remotes/origin/main", "from": "4c3ea3f…", "to": "9b1d2e7…" },
    { "name": "refs/remotes/origin/gone", "from": "70262e5…", "to": null }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `remotes` | The remotes asked, in order. |
| `moved[].name` | The ref's full name: `refs/remotes/…` or `refs/tags/…`. |
| `moved[].from`, `moved[].to` | What it named before and after; `null` for one that is new, or gone. |

## Exit status

`0` when the fetch went through, whatever it brought; `2` when it could not -
no remote, a remote git could not reach or get into.

## Options

| Option | Meaning |
| --- | --- |
| `REMOTE` | The remote to fetch from. |
| `--all` | Fetch from every remote. |
| `--prune` | Take away remote-tracking branches whose branch is gone from the remote. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [pull](/furca/reference/pull/) - fetch and bring what came into the branch.
- [remote](/furca/reference/remote/) - the remotes themselves, and `prune` without a fetch.
