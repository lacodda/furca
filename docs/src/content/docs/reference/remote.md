---
title: remote
description: List the remotes, or add, rename, point elsewhere, remove or prune one.
---

```sh
furca remote [-C PATH] [--json]
furca remote add NAME URL
furca remote rename FROM TO
furca remote set-url NAME URL
furca remote remove NAME
furca remote prune NAME
```

Each is `git remote` with the same verb, run through your own `git`. Without a
verb it lists the remotes with their addresses; a password written into an
address is left out, the user name stays.

```console
$ furca remote
mirror  https://git.example.com/acme/app.git
origin  git@github.com:acme/app.git

$ furca remote add backup /srv/git/app.git
Added backup; `furca fetch backup` brings its branches.
backup  /srv/git/app.git
mirror  https://git.example.com/acme/app.git
origin  git@github.com:acme/app.git
```

| Verb | What it does |
| --- | --- |
| `add` | Adds a remote. Nothing is fetched from it yet. |
| `rename` | Renames it: its remote-tracking branches move with it, and the branches that follow it follow it under the new name. |
| `set-url` | Points it at another address, for fetching and - unless a `pushurl` sends them elsewhere - for pushing. |
| `remove` | Removes it and its remote-tracking branches; a branch that followed it follows nothing. Nothing on the remote is touched. |
| `prune` | Asks the remote which of its branches are gone, and takes away their remote-tracking branches here. |

After a change the list is printed again as it now is. A name git will not
take, or a remote that is there already, is refused in git's words.

## JSON

`--json` prints the list after the change - for `prune`, what it took away:

```console
$ furca remote --json
[
  { "name": "mirror", "url": "https://git.example.com/acme/app.git", "push_url": null },
  { "name": "origin", "url": "git@github.com:acme/app.git", "push_url": null }
]

$ furca remote prune origin --json
{ "remotes": ["origin"], "moved": [{ "name": "refs/remotes/origin/gone", "from": "70262e5…", "to": null }] }
```

`push_url` is the address pushes go to when a `pushurl` sends them elsewhere,
and `null` when they go where fetches come from.

## Exit status

`0` when git did it; `2` when it refused.

## Related

- [fetch](/furca/reference/fetch/) - `--prune` prunes while it fetches.
- [push](/furca/reference/push/) - `--all-remotes` pushes to every one of them.
