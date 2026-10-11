---
title: push
description: Push the current branch to the remote it follows, and wait for its CI if asked.
---

```sh
furca push [-C PATH] [--wait-ci] [--json]
```

Pushes the current branch to the remote it follows - the same as
`git push <remote> HEAD:<branch>`, run through your own `git`, so hooks and
credential helpers work as they do in a terminal. A branch that follows
nothing is refused with a pointer to `git push --set-upstream`.

```console
$ furca push --wait-ci
  push       4c3ea3f to origin refs/heads/main
  ci         waiting for the runs to start
  ci         ci.yml running, docs.yml running
  ci         ci.yml passed, docs.yml passed
Pushed 4c3ea3f to origin main
  passed     ci.yml    push (main)  https://github.com/acme/app/actions/runs/1
  passed     docs.yml  push (main)  https://github.com/acme/app/actions/runs/2
CI is green.
```

With `--wait-ci` it then waits for the workflow runs the push started on the
branch, and stops waiting as soon as one fails. Progress goes to standard
error; the answer goes to standard output.

## Forges

The remote's address names the forge: `github.com` is GitHub (Actions), any
other host is Forgejo (Forgejo Actions, the `actions/runs` API of Forgejo 11
and later). In a repository with a [`release.toml`](/furca/reference/release/),
its `ci.provider` decides instead. An address over SSH says nothing about the
web, so the forge is taken to answer HTTPS on the same host.

The forge is asked with a token when there is one, and anonymously otherwise,
which a public repository allows at a lower rate:

| Forge | Token |
| --- | --- |
| GitHub | `GH_TOKEN`, `GITHUB_TOKEN`, or the `gh` CLI's own when it is signed in |
| Forgejo | `FORGEJO_TOKEN` |

With a token the runs are asked about every 15 seconds; without one, every
minute - GitHub answers sixty anonymous questions an hour. The runs have five
minutes to appear and two hours to finish. A token is sent, never printed.

## JSON

```console
$ furca push --wait-ci --json
{
  "commit": "4c3ea3f…",
  "remote": "origin",
  "branch": "main",
  "ci": {
    "outcome": "green",
    "runs": [
      { "id": 1, "workflow": "ci.yml", "name": "CI", "event": "push", "reference": "main", "state": "success", "url": "https://github.com/acme/app/actions/runs/1" }
    ]
  }
}
```

| Field | Meaning |
| --- | --- |
| `ci` | `null` without `--wait-ci`. |
| `ci.outcome` | `green` - every run ended and none failed; `red` - one failed or was cancelled; `missing` - no run started within five minutes (with `missing`, the workflows that never appeared); `timeout` - runs were still going after two hours. |
| `ci.runs[].state` | `pending`, `success`, `skipped`, `cancelled` or `failure`. |

## Exit status

`0` when the push went through and, with `--wait-ci`, CI is green; `1` when
CI is red, missing or still running; `2` when nothing could be pushed or the
forge could not be asked.

## Options

| Option | Meaning |
| --- | --- |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--wait-ci` | Wait for the CI runs the push starts. |
| `--json` | Print JSON instead of text. |

## Related

- [checks](/furca/reference/checks/) - the runs on a commit, without pushing.
- [release](/furca/reference/release/) - `furca release run` pushes and waits the same way.
