---
title: push
description: Push the current branch where it goes, to one remote or to every one, with a lease when forced, and wait for its CI if asked.
---

```sh
furca push [--remote NAME | --all-remotes] [--force-with-lease] [--wait-ci] [-C PATH] [--json]
```

Pushes the current branch through your own `git`, so hooks, its credential
helper and ssh-agent work as they do in a terminal; furca never asks for a
password. Where it goes:

- **By default**, where the branch is pushed: its `pushRemote`,
  `remote.pushDefault`, or the remote it follows - and with none of those, the
  only remote there is, or `origin`.
- **`--remote NAME`**, to that remote.
- **`--all-remotes`**, to every remote, one after another: a mirror on another
  forge gets what the main one gets. A remote that refuses does not stop the
  rest.

On the remote the branch follows it goes to the branch it follows there;
anywhere else, to a branch of its own name. A branch that follows nothing
follows the one it is pushed to from then on - the default remote's, with
`--all-remotes` - so a first push needs no `--set-upstream`. The annotated tags
on the history it pushes go with it (`--follow-tags`), so a release tag reaches
every remote the branch does; a plain tag stays.

```console
$ furca push --all-remotes
  push       4c3ea3f to mirror refs/heads/main
  push       4c3ea3f to origin refs/heads/main
Pushed 4c3ea3f to mirror main
Pushed 4c3ea3f to origin main
```

`--force-with-lease` replaces what the remote's branch holds, but only if it
still holds what the last fetch saw there: the lease names that commit outright,
so a fetch in the background cannot stretch it over work nobody here has seen.
If someone pushed since, git refuses and nothing is lost. There is no force
without a lease.

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
branch, and stops waiting as soon as one fails - on the remote named, or on the
one the branch goes to by default, not on each mirror. Progress goes to
standard error; the answer goes to standard output.

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
  "branch": "main",
  "pushed": [
    { "remote": "origin", "branch": "main", "to": "refs/heads/main", "commit": "4c3ea3f…", "forced": false, "upstream_set": false }
  ],
  "failed": [],
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
| `pushed[]` | Each remote the branch reached, in order: the branch there, the commit, `forced` with a lease, `upstream_set` when the branch follows it from now on. |
| `failed[]` | Each remote that refused, with git's words in `error`. |
| `ci` | `null` without `--wait-ci`. |
| `ci.outcome` | `green` - every run ended and none failed; `red` - one failed or was cancelled; `missing` - no run started within five minutes (with `missing`, the workflows that never appeared); `timeout` - runs were still going after two hours. |
| `ci.runs[].state` | `pending`, `success`, `skipped`, `cancelled` or `failure`. |

## Exit status

`0` when every push went through and, with `--wait-ci`, CI is green; `1` when
CI is red, missing or still running; `2` when a remote refused - after saying
which ones took it - or the forge could not be asked.

## Options

| Option | Meaning |
| --- | --- |
| `--remote <NAME>` | Push to that remote. |
| `--all-remotes` | Push to every remote, one after another. |
| `--force-with-lease` | Replace the remote's branch, if it holds what the last fetch saw. |
| `--wait-ci` | Wait for the CI runs the push starts. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |

## Related

- [fetch](/furca/reference/fetch/) - what the remotes have, before a push with a lease.
- [remote](/furca/reference/remote/) - the remotes a push goes to.
- [checks](/furca/reference/checks/) - the runs on a commit, without pushing.
- [release](/furca/reference/release/) - `furca release run` pushes and waits the same way.
