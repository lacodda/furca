---
title: rebase
description: Replay the current branch's commits on another branch, or by a todo list that reorders, rewords, folds and drops them; a conflict stops it halfway.
---

```sh
furca rebase ONTO [-C PATH] [--json]
furca rebase (ONTO | --root) --plan [-C PATH] [--json]
furca rebase (ONTO | --root) --todo FILE [-C PATH] [--json]
```

Replays the commits `HEAD` has and `ONTO` does not on top of `ONTO`, one by one,
through your own `git`, and moves the branch there. `ONTO` is looked for as a
checkout looks for a name: a full ref name as it is, else a local branch, a
remote-tracking branch, a tag - and else any commit. Changes not committed are put
away first and brought back after, by git's own autostash; no editor opens.

| Option | Meaning |
| --- | --- |
| `--plan` | Say which commits would be replayed, oldest first, and change nothing. |
| `--todo FILE` | Replay them as the JSON todo list in `FILE` says; `-` reads it from standard input. |
| `--root` | In place of `ONTO`: every commit `HEAD` has, from the first. With `--plan` or `--todo`. |

```console
$ furca rebase main
A rebase stopped halfway, 1 path in conflict:
  both modified  README.md
Resolve them with furca resolve, then furca continue - or furca abort.

$ furca conflicts
A rebase is halfway on feature/timeouts, at step 2 of 2: docs: say that requests time out.
  both modified  README.md
```

A rebase that meets a conflict has not failed: it is halfway, as a
[merge](/furca/reference/merge/) is. Resolve the files with
[`furca resolve`](/furca/reference/resolve/) - **ours** is the new base, **theirs** the
commit being replayed - then [`furca continue`](/furca/reference/continue/) goes on
with the next commit; `furca continue --skip` leaves the commit out, and
[`furca abort`](/furca/reference/abort/) puts everything back.

## By a plan

An interactive rebase is two steps here, and no editor: read the plan, then give back
what is to become of each commit.

```console
$ furca rebase HEAD~4 --plan
4 commits to replay on 1058da2 feat(log): say what the client does, oldest first:
  9316ba5 docs: say how to use it
  2f66d6a fix(retry): wait a quarter of a second, not a tenth
  3688f88 wip: warnings
  9ef333b docs: name the licence
```

A commit the branch's upstream has already is marked `(pushed)`: rewriting it parts
the branch from what was pushed, and it is pushed again with
[`--force-with-lease`](/furca/reference/push/).

A todo list is a JSON list in the order to replay the commits, each with its commit -
an id, or the start of one - and an action:

| Action | What becomes of the commit |
| --- | --- |
| `pick` | Replayed as it is. |
| `reword` | Replayed under `message`. Your hooks judge it, as they judge any commit. |
| `squash` | Folded into the commit before it, their two messages one after the other. |
| `fixup` | Folded into the commit before it, its own message left out. |
| `drop` | Left out. |

```json
[
  { "commit": "2f66d6a", "action": "pick" },
  { "commit": "9316ba5", "action": "reword", "message": "docs: show how to use it" },
  { "commit": "9ef333b", "action": "fixup" },
  { "commit": "3688f88", "action": "drop" }
]
```

```console
$ furca rebase HEAD~4 --todo todo.json
Replayed 2 commits: main is at 301f228.
```

The list names every commit of the plan once. One it leaves out is refused, not
dropped - in git's own todo list a deleted line loses a commit without a word - and
so is a commit named twice, one that is not in the plan, a `reword` with no message,
and a `squash` or `fixup` with no commit replayed before it. All of it is checked
before git runs, so a list that is wrong moves nothing:

```console
$ furca rebase HEAD~2 --todo short.json
furca: the plan leaves out 61989cf: a commit to leave out is named with drop
```

A range with a merge in it has no plan: a rebase would replay what the merge brought
in commit by commit and leave the merge out. Rebase the commits after it. See
[ADR 0024](https://github.com/lacodda/furca/blob/main/docs/adr/0024-a-rebase-is-given-its-plan-and-a-rewrite-in-place-happens-whole.md).

## JSON

A rebase answers in the same shape as [`merge`](/furca/reference/merge/#json); `made`
is how many commits were replayed. `--plan` answers with the plan:

| Field | Meaning |
| --- | --- |
| `branch` | The branch `HEAD` is on, which the rebase rewrites; `null` on no branch. |
| `onto` | The commit the steps are replayed on; `null` with `--root`. |
| `steps` | The commits to replay, oldest first: each with its `commit`, its whole `message`, and `pushed` - whether the branch's upstream has it already. |

## Exit status

`0` when the rebase is done, or the plan was read; `1` when it stopped halfway; `2`
when furca or git refused and nothing moved.

## Related

- [squash](/furca/reference/squash/) - the last commits made one, in one step.
- [absorb](/furca/reference/absorb/) - what is staged, put into a commit already made.
- [continue](/furca/reference/continue/) and [abort](/furca/reference/abort/) - the way on from a stop.
