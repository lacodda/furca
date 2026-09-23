---
title: status
description: Show where HEAD is - branch, commit and upstream.
---

```sh
furca status [-C PATH] [--json]
```

Opens the repository containing `PATH` (the current directory if omitted),
walking up through parent directories the way `git` does, and says where
`HEAD` is:

```console
$ furca status
On main at 171154d, tracking origin/main
```

A repository without commits reads `On main, no commits yet`; a detached
`HEAD` reads `HEAD detached at 3f1c2b9`.

The state of the working tree - changed, staged and untracked files - is not
part of `status` yet.

## JSON

```console
$ furca status --json
{
  "branch": "main",
  "commit": "171154d1429f569e4ac61e6587e610f36568af55",
  "detached": false,
  "upstream": "origin/main"
}
```

| Field | Meaning |
| --- | --- |
| `branch` | The branch `HEAD` is on; `null` when detached. |
| `commit` | The full id `HEAD` resolves to; `null` before the first commit. |
| `detached` | `true` when `HEAD` points at a commit rather than a branch. |
| `upstream` | The remote-tracking branch the current branch follows, such as `origin/main`; `null` if none is configured. |

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [refs](/furca/reference/refs/) - every branch and tag, not only the current one.
- [The engine](/furca/guides/the-engine/) - how the answer is computed.
