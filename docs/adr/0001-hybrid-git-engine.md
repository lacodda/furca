# 0001 — Hybrid git engine: gix for reads, the system git for mutations

Date: 2026-09-03
Status: Accepted

## Context

A git client needs two very different things from its engine: fast, frequent reads (log, status, diff, blame, refs — run on every keystroke, every file selection, every refresh) and correct, infrequent mutations (commit, merge, rebase, push, pull, stash — run on explicit user action).

Two engines were on the table:

1. **Everything through the system `git` CLI.** Simple and always correct — it is git — but every call spawns a process. On Windows that costs 30-60 ms per spawn, which is unnoticeable once but adds up across a status bar, a log view and a diff panel refreshing together.
2. **Everything through `gix` (gitoxide), an in-process Rust implementation.** Fast, but gitoxide does not implement the full surface of git — hooks, credential helpers, LFS, commit signing and the full breadth of `git config` resolution are either partial or absent. Reimplementing them is a standing tax and a correctness risk that never fully pays off.

## Decision

Split by operation kind. Reads go through `gix`, in-process, because a read cannot corrupt a repository and its cost is paid on every interaction. Mutations go through the system `git` CLI, because a mutation's correctness depends on the exact behavior real git installations already have configured: hooks that run on commit, a credential helper that already knows the user's tokens, LFS smudge/clean filters, GPG or SSH commit signing, and whatever else lives in global and local config.

The rule of thumb: if getting it wrong risks losing or corrupting history, it goes through `git`. If it only risks showing stale information for a moment, it goes through `gix`.

## Consequences

**Positive.** The common path — everything a client does just by being open — never spawns a process. Mutations behave exactly as they would from a terminal, because they run in one, so hooks, credential helpers, LFS and signing work without furca reimplementing any of them.

**Negative.** The engine has two code paths instead of one, and a feature that starts as a read (e.g., a live diff preview before staging) may need to fall back to `git` once it needs to touch the index. `furca-core` draws this line explicitly in its public API rather than leaving each caller to guess.
