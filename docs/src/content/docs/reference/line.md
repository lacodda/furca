---
title: line
description: Show where every repository stands - uncommitted, unpushed and unreleased - in one table.
---

```sh
furca line [PATH...] [--json]
```

Reads several repositories side by side and says, for each, what it holds
that has not gone anywhere yet: changes not committed, commits not pushed,
stashes, an operation left halfway - and how far it is from its last tag.

```console
$ furca line "$HOME/src/*"
REPOSITORY  BRANCH  UPSTREAM           TAG     TAGGED      SINCE  ONLY HERE
api         main    up to date         v1.4.0  2026-10-02      0  -
notes       main    3 ahead            v0.2.1  2026-09-28      5  2 changed, 1 untracked, 3 unpushed
tools       topic   -                  -       -               -  4 unpushed
web         main    1 ahead, 2 behind  v2.0.0  2026-10-11      1  1 unpushed, 1 stash

4 repositories: 1 settled, 3 with work only here.
```

Everything is read from this machine: nothing is fetched, so `behind` counts
what the last `git fetch` brought, not what the remote has now. The
repositories are read several at a time.

## Which repositories

With no `PATH`, the repositories the owner's
[record](https://lacodda.github.io/rigger/) keeps - every project of kind
`repo` that is not retired, under its project's name. Without rigger on the
machine, name the repositories instead.

Each `PATH` is one of:

- **a pattern** with `*` or `?`, such as `"$HOME/src/*"` or
  `"C:\src\*-server"`. It stands for the repositories at the directories it
  matches. A match must be a repository itself - `src/*` finds the
  repositories in `src`, not the one `src` may sit in - and a directory that
  is not one is passed over. `*` matches any run of characters within one
  directory name, `?` one; a leading `.` only matches a `.` written out. On
  Windows and macOS names match without case. Quote the pattern so that furca expands it
  rather than the shell: a shell hands over every match, and furca treats
  each one as a path it was asked to read. furca does not expand `~`; the
  shell expands `$HOME` inside double quotes.
- **a path** without them: the repository it is in, found by walking up as
  `-C` does for the other commands.

A repository named twice is read once. A pattern that matches no repository,
or a path that is in none, is a mistake on the command line: the command
stops with exit status 2 rather than reading the rest.

## What the columns say

| Column | Meaning |
| --- | --- |
| `BRANCH` | The branch `HEAD` is on, or `(detached)`. |
| `UPSTREAM` | The current branch against the remote-tracking branch it follows: `up to date`, `3 ahead`, `2 behind`, both, `gone` when that branch was deleted on the remote and pruned here, `-` when it follows nothing. |
| `TAG` | The nearest tag `HEAD` descends from, as `git describe --tags` picks it. |
| `TAGGED` | When that tag was made: the tagger's date for an annotated tag, the commit's for a plain one. |
| `SINCE` | Commits `HEAD` has that the tag does not - what has not been released. |
| `ONLY HERE` | What exists on this machine alone, or `-`. |

`ONLY HERE` counts:

- **changed** - tracked files whose content differs from `HEAD`, staged or
  not, each once;
- **untracked** - what `git status` lists as untracked, a directory holding
  nothing tracked counted once;
- **unpushed** - commits reachable from `HEAD` or any local branch that no
  remote-tracking branch reaches, what `git log --branches HEAD --not
  --remotes` lists; without remotes, the whole history;
- **stashes**, and a **merge**, **rebase**, **cherry-pick**, **revert**,
  **bisect** or **am** in progress.

A repository with none of these is **settled**: nothing is lost if this
machine is. Being behind, or having commits since the last tag, does not
unsettle it.

A repository that cannot be read - a recorded path that is gone, a damaged
repository - gets a row with its name and the reason, and does not stop the
others.

## JSON

```console
$ furca line --json
{
  "source": "record",
  "settled": false,
  "repositories": [
    {
      "name": "notes",
      "path": "/home/ada/src/notes",
      "settled": false,
      "standing": {
        "branch": "main",
        "commit": { "id": "f3e5b25…", "parents": ["…"], "author": { … }, "committer": { … }, "subject": "feat: link notes 5" },
        "upstream": { "name": "origin/main", "ahead": 3, "behind": 0 },
        "changed": 2,
        "untracked": 1,
        "unpushed": 3,
        "stashes": 0,
        "tag": { "name": "v0.2.1", "since": 5, "time": "2026-09-28T18:04:11+03:00", "seconds": 1790607851 },
        "operation": null
      }
    },
    { "name": "gone", "path": "/home/ada/src/gone", "settled": false, "error": "could not open the repository: …" }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `source` | `record` when the list came from the record, `paths` when from the command line. |
| `settled` | Every repository was read and is settled. |
| `repositories[]` | In the record's order, or in the order named with each pattern's matches sorted by name. |
| `repositories[].name` | The project's name in the record, or the repository's directory name. |
| `repositories[].path` | The repository's root, absolute. |
| `repositories[].settled` | Read, and nothing in it exists on this machine alone. |
| `repositories[].standing` | Where it stands; absent when it could not be read. |
| `repositories[].error` | Why it could not be read; absent when it was. |
| `standing.branch` | The branch `HEAD` is on; `null` when detached. |
| `standing.commit` | The commit `HEAD` resolves to, as [log](/furca/reference/log/#json) prints one; `null` before the first commit. |
| `standing.upstream` | `name`, `ahead`, `behind`; `ahead` and `behind` are `null` when the remote-tracking branch is gone; `null` when the branch follows nothing. |
| `standing.changed`, `untracked`, `unpushed`, `stashes` | The counts above. |
| `standing.tag` | `name`, `since`, `time` (RFC 3339 with its own offset) and `seconds` (Unix); `null` when no tag is in the history. |
| `standing.operation` | `merge`, `rebase`, `cherry-pick`, `revert`, `bisect`, `am`, or `null`. |

## Exit status

`0` when every repository was read and is settled, `1` when one holds work
only here or could not be read, and `2` when there was no list to read: no
rigger and no `PATH`, or a `PATH` that names no repository.

## Options

| Option | Meaning |
| --- | --- |
| `PATH...` | Repositories to read: patterns or paths. Defaults to the record's. |
| `--json` | Print JSON instead of text. |

## Related

- [status](/furca/reference/status/) - one repository's `HEAD`.
- [checks](/furca/reference/checks/) - whether CI is green on a commit.
