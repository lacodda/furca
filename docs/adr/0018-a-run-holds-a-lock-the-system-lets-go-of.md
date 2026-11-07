# 0018 — A run holds a lock the system lets go of

Date: 2026-11-07
Status: Accepted.

## Context

ADR 0015 left this open: a window and a terminal, or two terminals, can run one repository's release at the same moment, because the checkpoint says where a run got to and not whether one is going. It is worse than two runs racing. A second run finds a checkpoint with no commit in it, takes the edits of the first for ones an interrupted run left behind, and puts them back underneath it.

The commit panel added another way in: whatever is staged goes into the release's commit, so a click on Stage during the gate puts a file in the release.

The usual lock is a file with a process id in it, checked against the processes alive. It has to be cleared when a run is killed, it is wrong when the id has been reused, and it needs a way of asking about a process on each system.

## Decision

**A run holds an operating-system lock on `.git/furca-release.lock` for as long as it goes.**

- `File::try_lock` is `LockFileEx` on Windows and `flock` elsewhere. The system lets go when the process ends, however it ended, so a killed run leaves nothing to clear and there is no id to be reused. The file stays behind, empty and harmless; removing it would open a race between two runs that opened it.
- It is taken before the checkpoint is read. A second run stops at `plan`, and says a run is going, before it has touched anything.
- `furca_release::running` asks without taking it, for whoever needs to hold off: the window refuses to change the index while a release is going in any door.
- A file system with no locks does not stop a release. The lock is there against a mistake, and the checkpoint still says where things stand.
- Programs a run starts do not inherit the lock, so a gate that outlives its run cannot hold up the next one.

## Consequences

**Positive.** Two doors on one release cannot undo each other. A crash needs no cleaning up, and nothing in the engine knows about processes.

**Negative.** `File::try_lock` needs Rust 1.89, so `furca-release` and the CLI say that, a step above the core's 1.88. Library users of `furca-release` move with them.
