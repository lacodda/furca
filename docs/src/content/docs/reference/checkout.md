---
title: checkout
description: Check out a branch - a remote one as a local branch that follows it - or, with --detach, a commit on no branch.
---

```sh
furca checkout BRANCH [-C PATH] [--json]
furca checkout --detach REVISION [-C PATH] [--json]
```

Checks out through your own `git` (`git switch`), so a `post-checkout` hook
runs as it does in a terminal. Changes in the working tree go along where they
do not meet the files being checked out; where they do, git refuses in its own
words and nothing moves.

What `BRANCH` means is decided before git is asked:

- **A local branch** is checked out.
- **A remote-tracking branch**, such as `origin/feature/login`, is the local
  branch `feature/login`: made and set to follow it when there is none yet,
  and checked out as it is when there is one that follows it. A
  `feature/login` here that follows something else, or nothing, is refused -
  checking it out would not be checking out the remote's.
- **Anything else** - a commit, a tag, `HEAD~1` - is refused. git would check
  it out on no branch, where a commit made next belongs to nothing; furca does
  that only when asked, with `--detach`.

```console
$ furca checkout topic
Checked out topic at 7012074.

$ furca checkout origin/feature/login
Checked out feature/login, a new branch at f1380f3 following origin/feature/login.

$ furca checkout HEAD~1
furca: HEAD~1 is no branch here: a commit or a tag is checked out on no branch, detached, and only when that is asked for

$ furca checkout --detach HEAD~1
HEAD is on f1380f3, on no branch.
```

To make a branch somewhere else and check it out at once,
[`furca branch create NAME FROM --checkout`](/furca/reference/branch/).

## JSON

```console
$ furca checkout main --json
{
  "branch": "main",
  "commit": "70120747…",
  "created": false,
  "upstream": "origin/main"
}
```

| Field | Meaning |
| --- | --- |
| `branch` | The branch `HEAD` is on now; `null` after `--detach`. |
| `commit` | The commit `HEAD` is on. |
| `created` | The branch was made by this checkout. |
| `upstream` | The remote-tracking branch it follows, or `null`. |

## Exit status

`0` when `HEAD` moved; `2` when furca or git refused.

## Related

- [branch](/furca/reference/branch/) - makes, renames and deletes branches, and sets what they follow.
- [status](/furca/reference/status/) - where `HEAD` is, and what the working copy holds.
