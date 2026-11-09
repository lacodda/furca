# 0021 — A remote is reached by git, and heard as it goes

Date: 2026-11-09
Status: Accepted.

## Context

v0.14 fetches, pulls and pushes from all three doors, and adds, renames, points elsewhere, removes and prunes remotes. Talking to a remote is where a git client most often grows a second git: its own transport, its own credential store, its own idea of where a branch goes. Each is a way to differ from the terminal - a token the client stored and the terminal never sees, a push that sets no upstream, a lease that follows a background fetch.

A run that talks to a remote also takes time a person waits through, and the window should say how far it has got, not only that it is busy.

## Decision

**Every one of them is a run of the system git through `git_cmd` (ADR 0001), and furca keeps no credential.**

- **Credentials are git's.** git's credential helper and ssh-agent answer as in a terminal; the environment is passed on as it is. furca never asks for a password: `GIT_TERMINAL_PROMPT=0` makes a missing credential fail at once rather than wait at a prompt nobody sees, and a failure whose words say git could not get in - no credential for an https address, no key the server took, a host ssh does not know - is `Error::Access` with a hint at where git looks. A merge a pull makes takes git's own message (`GIT_MERGE_AUTOEDIT=no`): an editor opened with no terminal would wait for ever.
- **Progress is read from git's standard error as it is written.** Given `--progress`, git rewrites a line with `\r` as each phase goes; the core reads the pipe as it fills, reads each line by its shape - a phase, a colon, a count or a percentage with a count - so a git speaking another language is read the same, and hands it to whoever asked. What the console keeps is what a terminal shows when git is done: each line as its last rewrite left it.
- **What moved is read from the refs, not from git's words.** A fetch and a prune compare the remote-tracking branches and tags before and after, in-process; git's summary follows its version and its language.
- **Where a branch goes is the core's one answer** (`Repository::destinations`): to the remote its `pushRemote`, `remote.pushDefault` or upstream names, or the only remote, or `origin`; on the remote it follows, to the branch it follows; anywhere else, to a branch of its own name. A branch that follows nothing is set to follow the one it is pushed to, so a first push needs no `--set-upstream` to be typed.
- **Annotated tags go with the branch** (`--follow-tags`, as GitHub Desktop pushes): a release tag reaches every remote the branch does, a mirror included. A plain tag is a bookmark and stays.
- **A force push is always a lease, and the lease names what it expects** (`--force-with-lease=<branch>:<id>`, the remote-tracking branch's id read just before the run). An implicit lease follows the remote-tracking branch, which a background fetch moves; then the lease holds over work never seen. There is no force without a lease.
- **Every remote at once is one remote after another.** A refusal from one does not stop the rest, and each is said apart.
- **A pull is git's pull.** It reconciles as `pull.rebase` and `pull.ff` say, or as asked with git's own flags - `--no-rebase`, `--rebase`, `--ff-only`; a conflict is left as git leaves it.
- **In the window, one run on a remote goes at a time.** A fetch moves only remote-tracking branches, so staging goes on beside it; a pull changes the index and queues with staging. While a release runs, a push, a pull and a change to the remotes are refused - the release pushes and tags itself - and a fetch is not.

## Consequences

**Positive.** What works in a terminal works in each door, with the same helper and the same keys; nothing furca stored can go stale or leak. A first push of a new branch, a push to a mirror and a forced push each have one way, and the console shows each as a command one could type.

**Negative.** A key with a passphrase that is not in ssh-agent, or a host whose key was never accepted, cannot be answered from the window: furca says where to do it, once, in a terminal. The progress words are git's, in git's language, beside the window's English. A handle opened before a remote was added, renamed or removed still reads the old list: gix reads the configuration once, so the doors open the repository again after such a change.
