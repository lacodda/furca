# 0007 — A release run is redone before its commit and resumed after it

Date: 2026-10-11
Status: Accepted. Carries out the pipeline of ADR 0003 in the crate of ADR 0006.

## Context

`furca release plan` (ADR 0006) says what the next release is and changes nothing. The run has to change things, and the things it changes are of two kinds:

- **Local and cheap:** rewriting the manifests and the texts, adding the changelog section, running the gate, making the release commit. Nothing has left the machine; anything can still be undone.
- **Remote and slow:** pushing, waiting minutes for CI, tagging, waiting for the publish workflow and the registries, telling the owner's record. These leave the machine, cannot be taken back (ADR 0003: nothing pushed is rewritten, a tag never moves), and are exactly the steps a laptop going to sleep or a red CI run interrupts.

A single "resume from the step that stopped" for both halves looks simpler and is wrong for the first: when the gate fails, the fix is a code change made on top of the uncommitted release edits. Resuming would sweep the fix into `chore(release): vX.Y.Z` and leave it out of the changelog section rendered before it existed.

## Decision

**Before the commit, a run is redone; after it, it is resumed.**

- A stop before the release commit - a red gate, a refused bump - puts every file the run wrote back to the bytes it held before, and removes the checkpoint. The next `run` starts from a fresh plan, which sees the fix as one more commit of the release.
- A run killed before its commit (no chance to clean up) leaves its checkpoint; the next `run` or `resume` takes its edits back first - only where the files still hold exactly what the run wrote, so a person's later edit is never overwritten - and starts over.
- After the commit, the checkpoint records each step as it completes: the commit, what was pushed, the commit CI passed on, the tag, the registries reached. `resume` carries on from the first step not done. `run` refuses while such a release is under way.

The checkpoint is `.git/furca-release.json`: inside `.git`, so it is never committed, survives between runs, and belongs to one repository. It is written aside and renamed into place, so a kill mid-write leaves the previous checkpoint, never half of one.

The rest of the run's shape follows from the same split:

- **The tag names the commit CI passed on**, which is the release commit or a later fix on top of it, never a commit CI did not pass on. A skipped CI run proves nothing and does not count as green.
- **What the registries hold decides whether the publish is done**, not the workflow's colour: a publish re-run by hand starts from another commit and the tag's own runs never show it.
- **A tag cannot create a package**: crates.io trusted publishing and npm OIDC publish only what exists. A package its registry has never seen stops the run before the tag, with the command to publish it by hand from the commit about to be tagged.
- **A gate checks; it does not edit.** A tracked file the gate changed besides the release's own edits stops the run, since the release commit would leave it behind. The bump rewrites a Cargo workspace's tracked `Cargo.lock` with the manifests for the same reason: the first build would otherwise change it.
- **Every git operation goes through `furca-core`'s git door** (ADR 0001), and its command log is part of the report.
- **The CI of the forge** (GitHub Actions, Forgejo Actions) is read by a `hosting` module inside `furca-release`, not by a crate of its own: every caller already depends on the engine, and a new crate means a first publish by hand and a trusted publisher only the owner can set up. It moves out when a caller without the engine appears.

## Consequences

**Positive.** A red gate costs a rerun, never a cleanup by hand; a red CI or a failed publish costs a fix and `resume`, never a second tag or a moved one. The invariants of ADR 0003 hold whoever calls - the CLI, a CI job, later the window - because they are the run's, not the caller's.

**Negative.** A gate that fails at its last command reruns from the top after the fix, the bump and the changelog included: cheap steps, but the gate itself can be long. A fix after a red CI lands after the release commit and outside the changelog section that commit carries; the section can be amended by hand in the fix, and the engine does not do it for you. The run needs network for the remote half and a token for an unthrottled forge.
