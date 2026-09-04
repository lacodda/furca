# 0003 — The release engine lives inside the client

Date: 2026-09-03
Status: Accepted

## Context

Releasing this product line follows a standing shape: propose the next version from Conventional Commits history, run a consistency gate across manifests, generate a changelog, run gate commands, commit, push, wait for CI to go green, tag, check the target registries, run post-release steps. Today that shape is repeated by hand or by ad hoc scripts, project by project.

Furca already has to know about git refs, remotes and history to be a git client at all. It is also, structurally, the same kind of program a dedicated "release tool" would be — the difference is only which git operations it performs and in what order.

## Decision

The release pipeline lives inside `furca-core` and is exposed as `furca release plan | run | resume`:

- `plan` proposes the next version from Conventional Commits since the last tag, without changing anything.
- `run` executes the pipeline: consistency gate, changelog via `git-cliff-core`, gate commands, commit, push, wait for green CI, tag, registry checks, post steps.
- `resume` picks the pipeline back up after an interruption (a failed gate, a red CI run) without repeating steps already done.

Invariants the engine holds regardless of caller:

- A tag is created only after CI is green for the commit it points at — never before.
- Pushed history is never rewritten.
- `CHANGELOG.md` is never regenerated with `-o` (full overwrite); only the unreleased section is turned into a dated one.
- The next version is always the next free number — the engine does not let a caller retag or reuse one.

## Consequences

**Positive.** A separate release tool would have to relearn everything furca already knows about the repository it is releasing — its remotes, its CI provider, its tag scheme — for no benefit, since furca is present anyway. Building it into the client the assistant already drives means one invocation triggers the whole pipeline, and `resume` makes a red CI run or a failed gate recoverable instead of forcing a restart from scratch.

**Negative.** `furca-core` now owns a domain (release orchestration) that has nothing to do with reading or mutating a working tree, which is a second responsibility for one crate to carry. The four invariants above are load-bearing specifically because this pipeline runs unattended from a terminal: nothing catches a mistake before it is pushed except the engine's own rules.
