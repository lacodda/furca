---
title: branch
description: List the local branches, or make, rename or delete one, or set what it follows.
---

```sh
furca branch [-C PATH] [--json]
furca branch create NAME [FROM] [--checkout]
furca branch rename FROM TO
furca branch delete NAME [--force]
furca branch follow NAME UPSTREAM
furca branch unfollow NAME
```

Each runs your own `git`, so hooks and your configuration apply as in a
terminal - except where furca has already decided, which is said below.
Without a verb it lists the local branches as [`furca refs`](/furca/reference/refs/)
does:

```console
$ furca branch
  f1380f3 feature/login -> origin/feature/login
* 7012074 main -> origin/main (1 ahead)
  7f29a8e topic
```

| Verb | What it does |
| --- | --- |
| `create` | Makes a branch at `FROM` - a commit, a tag, another branch; `HEAD` when left out - and leaves `HEAD` where it is. `--checkout` checks it out once it is made. |
| `rename` | Renames a branch. What it follows and its reflog go with it, and `HEAD` with them when it is on it. |
| `delete` | Deletes a branch. One holding commits that no other branch or tag has is refused with how many, unless `--force`. |
| `follow` | Has a branch follow another - a remote-tracking branch such as `origin/main`, or a local one: what [`pull`](/furca/reference/pull/) pulls from and [`push`](/furca/reference/push/) pushes to. |
| `unfollow` | Has a branch follow nothing. Its next push sets it to follow its own name there. |

```console
$ furca branch create topic
Made topic at 7012074.

$ furca branch rename feature/login login
Renamed feature/login to login.

$ furca branch follow login origin/feature/login
login follows origin/feature/login now.
```

## What a new branch follows

A new branch follows nothing - whatever `branch.autoSetupMerge` says - except
when it is made from a remote-tracking branch and takes that branch's name
there: `furca branch create feature/login origin/feature/login` follows
`origin/feature/login`, which is what [checking out](/furca/reference/checkout/)
a remote branch does. A branch made from `origin/main` as `fix` would
otherwise follow `main`, and its first push would go there. A branch that
follows nothing follows its own name on the remote it is first pushed to.

## What a deletion loses

`delete` counts the commits only the branch holds: the ones no other branch,
remote-tracking branch or tag reaches, nor `HEAD`. With none, the branch goes.
With some, it is refused and says how many:

```console
$ furca branch delete topic
furca: topic holds 1 commit that no other branch or tag has: deleting it anyway loses it

$ furca branch delete topic --force
Deleted topic, which was at 7f29a8e; the 1 commit only it held went with it, and git's reflog keeps it a while.
```

This is not `git branch --delete`'s own rule, which asks only whether the
branch is merged into its upstream or into `HEAD`: a branch a tag keeps loses
nothing, and is deleted without `--force`. The commit it was at is said, so
`furca branch create topic 7f29a8e` makes it again. The branch `HEAD` is on is
refused either way: check out another one first.

## JSON

For `create`, `rename`, `follow` and `unfollow`, `--json` prints the branch as
it now is, in the shape [`furca refs --json`](/furca/reference/refs/#json) gives
each of its `branches`; without a verb, the list of them. `delete` prints what
went:

```console
$ furca branch delete topic --force --json
{
  "name": "topic",
  "target": "7f29a8e…",
  "lost": 1
}
```

| Field | Meaning |
| --- | --- |
| `target` | The commit the branch was at. |
| `lost` | The commits only it held, which nothing names now; `0` for a branch merged or kept elsewhere. |

## Exit status

`0` when it was done; `2` when git or furca refused - a name git will not
take, a branch of the new name already there, a deletion that would lose
commits.

## Related

- [checkout](/furca/reference/checkout/) - checks a branch out, or a commit on no branch.
- [tag](/furca/reference/tag/) - the other names in the history.
