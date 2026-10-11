---
title: checks
description: Show the CI runs on a commit and how they ended.
---

```sh
furca checks [REVISION] [-C PATH] [--json]
```

Asks the forge the current branch's remote points at for the workflow runs on
a commit - `HEAD` unless a revision names another: an id, a branch, a tag.
For each workflow, event and branch or tag it shows the newest run.

```console
$ furca checks
CI on 4c3ea3f
  passed     ci.yml    push (main)  https://github.com/acme/app/actions/runs/101
  failed     docs.yml  push (main)  https://github.com/acme/app/actions/runs/102
Red.
```

A branch that follows no remote is read against `origin`. Which forge, and
which token it is asked with, is the same as for
[push](/furca/reference/push/#forges).

## JSON

```console
$ furca checks --json
{
  "commit": "4c3ea3f…",
  "runs": [
    { "id": 101, "workflow": "ci.yml", "name": "CI", "event": "push", "reference": "main", "state": "success", "url": "https://github.com/acme/app/actions/runs/101" }
  ],
  "green": true
}
```

| Field | Meaning |
| --- | --- |
| `runs[].workflow` | The workflow's file name. |
| `runs[].reference` | The branch or tag the run was started for. |
| `runs[].state` | `pending`, `success`, `skipped`, `cancelled` or `failure`. |
| `green` | At least one run, every run ended, and none failed or was cancelled. |

## Exit status

`0` when the commit is green, `1` when it is not - a run failed, is still
going, or there are none - and `2` when the forge could not be asked.

## Options

| Option | Meaning |
| --- | --- |
| `REVISION` | The commit to read. Defaults to `HEAD`. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [push](/furca/reference/push/) - push, then wait for these runs.
