# 0004 — History order is topological, time only breaks ties

Date: 2026-09-23
Status: Accepted. The order stands; its implementation is replaced by ADR 0005.

## Context

`furca-core` hands the history to three doors, and the window draws it as a graph: one row per commit, edges from each commit down to its parents. That drawing is only correct if no parent is listed before any of its children.

Ordering by commit time almost gives that, and gitoxide's simple walk does it cheaply. It breaks as soon as clocks disagree: a machine with a slow clock records a child as older than its parent, and once a merge queues both at once, a time-sorted walk lists the parent first. The graph then has an edge pointing up. Nothing in the data is wrong; the order is.

## Decision

`Repository::log` walks with gitoxide's topological walk in date order — the order of `git log --date-order`: a parent never before any of its children, newer commits first within that rule. The walk uses the repository's commit-graph file when one exists.

Each commit is described after the walk has ordered it, so every commit is read twice. The repository handle carries an object cache (4 MiB) so the second read does not decompress again.

The `Tips` passed to `log` say where the walk starts: `HEAD` only (`git log`) or every ref as well (`git log --all`, what a graph shows). Duplicates among the tips are left to the walk, which skips a tip it has already seen.

## Consequences

**Positive.** Every consumer — the CLI, the window, a future MCP tool — gets an order that draws as a graph, and a test with a backwards clock proves it: replacing the walk with a time-sorted one makes it fail.

**Negative.** A topological walk has to know a commit's children before it can emit the commit. Without a commit-graph file that means reading more history than the limit asks for. The speed budget for the first 500 commits is measured in the benchmark, not assumed here; if it fails, the fix is in the walk (commit-graph, generation numbers), not a return to time order.
