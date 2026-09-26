# 0005 — A lazy walk over generation numbers

Date: 2026-09-23
Status: Accepted. Replaces the implementation chosen in ADR 0004; the order it defines stands.

## Context

ADR 0004 fixed the order of the graph: `git log --date-order`, a parent never before any of its children. It implemented that with gitoxide's topological walk.

The first speed budget - open a repository and read the first 500 commits of its graph, from every branch and tag, in under 100 ms on 100 000 commits - measured 350 ms with a commit-graph and 1.8 s without. Two causes:

- gitoxide's walk counts every commit's children before it lists anything, down to the oldest generation among the starting points. With every tag as a starting point, the oldest tag sits near the root, so the whole history is counted for the first 500 rows.
- Peeling each ref through `peel_to_id` opens every annotated tag object, although `git pack-refs` already recorded the peeled id in `packed-refs`.

## Decision

`furca-core` walks the graph itself (`src/walk.rs`), lazily:

- Commits are explored in falling generation number, and only as deep as the next commit to list requires: a commit is listed once every reachable commit with a generation at or above its own has been explored and none of them is an unlisted child. A child always has a higher generation than its parent, so that is exact.
- Among the commits that may be listed, the newest committer date goes first; ties go to the one that became listable first. That reproduces git's order, ties included, and a test holds it to `git log --all --date-order` commit for commit on a 3 000-commit history with merges, open branches, tags, slow clocks and same-second commits, with and without a commit-graph.
- Generation numbers and parents come from the commit-graph file when the commit is in it. A commit it does not hold - made after the file was written, or every commit when there is no file - counts as infinitely high, as it does in git.
- Refs are peeled through the packed-refs buffer (`peel_to_id_packed`).

## Consequences

**Positive.** The budget holds with room: a median of 20-30 ms on the author's machine against 100 ms. The order is proven equal to git's, not argued.

**Negative.** Without a commit-graph every commit is infinitely high, the walk explores the whole history before listing anything, and the first 500 rows cost about as much as they do in git (one to two seconds on 100 000 commits). That layout is measured and published but not promised. The fix is not a cleverer walk - exactness needs the bound - but a commit-graph: git writes one on `gc`, and the desktop app will have git write one when a repository lacks it.

The walk is ours to maintain now, not gitoxide's. It is about 250 lines, and the order test and the budget benchmark fail on a change to its ordering or its laziness respectively.
