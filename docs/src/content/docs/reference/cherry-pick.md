---
title: cherry-pick
description: Apply the change commits made onto the current branch, each as a new commit with its own message.
---

```sh
furca cherry-pick REVISION... [-C PATH] [--json]
```

Applies the change each commit made, in the order given, onto the branch `HEAD` is
on - each as a new commit with the commit's own message, through your own `git`.
Each revision is resolved to its commit before git is asked, so a branch that moves
meanwhile does not change what was picked. A merge commit is picked against its
first parent: what it brought into the line it was merged into.

A commit `HEAD` already has is refused - picking it again changes nothing - and so is
a pick while another operation is halfway.

```console
$ furca cherry-pick topic/docs
Picked 1 commit: main is at 3e4958d.

$ furca cherry-pick 653ce97
furca: 653ce97 is in the history of HEAD already: picking it again changes nothing
```

A pick that meets a conflict stops halfway, as a [merge](/furca/reference/merge/)
does: resolve the files, then [`furca continue`](/furca/reference/continue/) makes
the commit and goes on with the next one; `furca continue --skip` leaves it out;
[`furca abort`](/furca/reference/abort/) puts everything back. A pick whose change is
already there - every conflict resolved with our side - comes out empty, and git
refuses to make it: skip it.

## JSON

The same shape as [`merge`](/furca/reference/merge/#json); `made` is how many commits
the pick made.

## Exit status

`0` when every commit was picked; `1` when it stopped halfway; `2` when furca or git
refused and nothing moved.
