# 0017 — CI is the gate

Date: 2026-11-06
Status: Accepted.

## Context

`gate.commands` ran the whole gate on the machine the release ran from: clippy, the tests, both benchmarks, the build of the window and of the site. That takes more memory than a developer's machine can spare beside what else it has open. The run was killed part-way at v0.10 and again at v0.11, and each time a release with nothing wrong in it stood waiting for hours.

It also proved less than it cost. It covered one system out of three, and CI ran the same commands again before the tag.

## Decision

**The full gate runs in CI. `gate.commands` holds only what takes seconds and compiles nothing: `cargo fmt --all --check`.**

- The `ci` step, which has always stood between the push and the tag, is the judge. What comes back red is fixed, pushed and picked up with `furca release resume`.
- CI gains the one command it did not run before the tag: the build of the site, as a `docs` job in `ci.yml`. Nothing the old gate checked is lost.
- The engine does not change. `gate.commands` still has to name something, because a repository with no CI needs it; what it names is each repository's choice.

## Consequences

**Positive.** A release no longer needs a free machine. Nothing rebuilds `target/` during a run, so the window and the CLI no longer have to run from a copy outside it.

**Negative.** A stop before the commit took the release's edits back; most failures now come after the push, so a release commit can stand on `main` with red CI until the fix follows it. The tag still goes on green only. A mistake costs a round of CI, some forty minutes, rather than a local run.

## Alternatives

- **Free the machine before each gate.** It worked until the next session opened something; the wait is the problem, and the owner is who it waits for.
- **Run the gate in parts, in the foreground.** The same memory, spent more slowly.
- **A branch and a pull request for the release commit.** It would keep `main` green, but the run pushes to `main` by design, and a second path through the engine is more to keep right than a red commit followed by its fix.
