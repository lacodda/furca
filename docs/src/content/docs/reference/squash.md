---
title: squash
description: Make one commit of a commit and every commit after it up to HEAD, in one step that cannot conflict.
---

```sh
furca squash REVISION [-m MESSAGE] [-C PATH] [--json]
```

Makes one commit of `REVISION` and every commit after it, up to `HEAD`, in the place
of `REVISION`. The one commit holds exactly what `HEAD` held, so nothing can
conflict; it keeps the author of `REVISION`, and your hooks run.

| Option | Meaning |
| --- | --- |
| `-m`, `--message MESSAGE` | The message of the one commit. Without it: the commits' messages one after another, oldest first. |

```console
$ furca squash HEAD~1
Squashed into one commit: main is at b15cab2.

$ furca show --json | jq -r .message
fix(retry): wait a quarter of a second, not a tenth

docs: show how to use it
```

`REVISION` is a commit of the line `HEAD` came by, with no merge from it on. A squash
waits while something is staged - it would go into the commit unasked - and one your
`commit-msg` hook refuses leaves the branch where it was. What is not staged stays in
the working tree.

```console
$ furca squash HEAD
furca: b15cab2 is the commit HEAD is on: there is nothing after it to squash into it
```

Folding commits that are not the last ones, or keeping some apart, is a
[rebase by a plan](/furca/reference/rebase/#by-a-plan).

## JSON

The same shape as [`merge`](/furca/reference/merge/#json); `made` is `1`.

## Exit status

`0` when the commits are one; `2` when furca or git refused and nothing moved.
