---
title: pull
description: Pull into the current branch from the branch it follows, the way your configuration says or the way you ask.
---

```sh
furca pull [--merge | --rebase | --ff-only] [-C PATH] [--json]
```

Runs `git pull` for the current branch, from the branch it follows, through
your own `git`: its credential helper and ssh-agent answer for it. Without a
flag it puts the two together as your configuration says - `pull.rebase`,
`pull.ff` - and refuses as git refuses when that says nothing and the two have
parted. The flags are git's own:

| Flag | git's flag | What it does |
| --- | --- | --- |
| `--merge` | `--no-rebase` | Merges what came, with git's own message. |
| `--rebase` | `--rebase` | Replays the branch's own commits on what came. |
| `--ff-only` | `--ff-only` | Only fast-forwards, and refuses when the two have parted. |

```console
$ furca pull
Pulled origin/main into main: 4c3ea3f..9b1d2e7

$ furca pull
main is up to date with origin/main
```

A merge never opens an editor: it takes the message git writes. A pull that
meets a conflict stops with git's words and leaves the conflict in the files,
as git leaves it. A branch that follows nothing is refused - push it once and
it will.

## JSON

```console
$ furca pull --json
{
  "branch": "main",
  "upstream": "origin/main",
  "from": "4c3ea3f…",
  "to": "9b1d2e7…"
}
```

`from` and `to` are the commit `HEAD` was on before and after; they are the
same when nothing came.

## Exit status

`0` when the pull went through; `2` when it did not - no upstream, a refusal, a
conflict, a remote git could not reach.

## Options

| Option | Meaning |
| --- | --- |
| `--merge` | Merge, whatever the configuration says. |
| `--rebase` | Rebase the branch's own commits on what came. |
| `--ff-only` | Only fast-forward. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [fetch](/furca/reference/fetch/) - bring the remote's branches without touching yours.
- [push](/furca/reference/push/) - the other way.
