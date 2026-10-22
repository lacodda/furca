# 0010 — The window has git write the commit-graph

Date: 2026-10-22
Status: Accepted. Closes the negative consequence of ADR 0005.

## Context

The graph's first 500 rows cost 20-30 ms on 100 000 commits with a commit-graph file and one to two seconds without one (ADR 0005). Exactness needs the bound the file's generation numbers give: a commit the file does not hold counts as infinitely high, as it does in git, so every commit made since the file was written is explored before anything is listed. git writes the file on `gc`, which a busy repository runs rarely and a fresh clone has not run at all - so the slow case is exactly a repository someone has just started working in.

ADR 0001 keeps every change to a repository in the system `git`. The commit-graph is a cache git itself writes and reads; it changes nothing a person can see in their history.

## Decision

**When the window opens a repository whose commit-graph is missing or far behind, it has git write one, in the background:** `git commit-graph write --reachable --split`, through `git_cmd` like every other run, logged.

- **"Far behind" is a number in the core.** `Repository::commit_graph()` reads the file and counts the commits reachable from any ref that it does not hold, up to a cap. Missing, or 1 000 commits outside, is worth writing; fewer is a few milliseconds of exploration and not worth a process. The window asks; the CLI does not, because a command that only reads must not write.
- **A repository that turned the file off is left alone.** `core.commitGraph = false` is the person's choice, and furca neither reads the file there nor writes one.
- **`--split` adds a layer** rather than rewriting the whole file, the way `git maintenance` does, so a second write on a large history costs what is new, not what is old.
- **It is never in the way.** The write runs off the UI thread, one at a time per repository; the status line says it is running and what it did, and a failure is shown there and changes nothing else.

## Consequences

**Positive.** "Slow graph on a repository nobody has gc'd" stops being a class of report: the first open pays one `git` run, every open after it reads in milliseconds. The budget stays promised on the layout git produces.

**Negative.** furca writes a file inside `.git` that the person did not ask for. It is the file git writes on its own `gc`, in git's format, by git, and the command log shows it - but it is a write, and the docs say so.
