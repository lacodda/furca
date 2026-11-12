# 0022 — A checkout is decided by the core and made by git

Date: 2026-11-12
Status: Accepted.

## Context

v0.15 makes, checks out, renames and deletes branches, sets what a branch follows, checks out a commit on no branch, and makes, deletes and pushes tags - from all three doors.

Each of these is a single `git` command, and each of them asks git a question it answers from configuration the person may never have looked at. `git switch feature` creates a branch that follows `origin/feature` when there is one, and checks out nothing when there are two. `git branch fix origin/main` has `fix` follow `origin/main` - and then a push of `fix`, which goes where the branch follows (ADR 0021), would go to `main`. With `branch.autoSetupMerge=always` every new branch follows where it started. `git branch --delete` refuses a branch that is not merged into its upstream or `HEAD`, even when a tag keeps every commit it holds, and lets through one merged into an upstream that is about to be pruned. `git tag NAME` under `tag.gpgSign` wants a message and fails.

## Decision

**The core decides; git is told exactly that.** Every run goes through `git_cmd` (ADR 0001), so hooks such as `post-checkout` and the person's configuration apply - except where the configuration would answer the question the core already answered.

- **What a checkout of a name means is one function** (`Repository::checkout_of`): a local branch is checked out; a remote-tracking branch such as `origin/feature` is the local branch `feature` - made to follow it when there is none, checked out as it is when it follows it already, and refused when a `feature` here follows something else. Anything that is not a branch is refused: a commit or a tag is checked out on no branch only when that is asked for by name (`checkout_detached`, `--detach`). git is run with `--no-guess`, so it does not make its own guess.
- **A new branch follows only the remote branch whose name it takes.** `--track` when it is made from `origin/feature` as `feature` - which is what a checkout of a remote branch is - and `--no-track` otherwise, whatever `branch.autoSetupMerge` says. A branch that follows nothing is set to follow its own name on the remote it is first pushed to (ADR 0021), so a new branch never pushes into another one by accident. The start is given to git by its full name (`refs/remotes/origin/feature`), so a tag or a local branch of the same short name does not stand in for it.
- **A deletion is judged by what it would lose** (`Repository::only_on`): the commits the branch reaches that no other branch, remote-tracking branch or tag reaches, nor `HEAD`. None, and it goes; some, and it is refused with how many - `Error::Unmerged` in the core, a question with the number in the window, `--force` in the CLI. git is then run with `--delete --force`: its own rule has been answered with the history's facts. What was deleted comes back with the commit it was at, which is what it takes to make it again.
- **A plain tag is a bookmark and an annotated tag is a release.** With a message the tag is annotated - its message on git's standard input, signed when `tag.gpgSign` says so - and a push of its branch takes it along (`--follow-tags`, ADR 0021). Without one it is a plain pointer, made with `--no-sign`, that stays until pushed by name. `Git::tag` takes the message as an `Option`; the release engine's tag is annotated, as it was.
- **A tag pushed by name goes where the branch goes** (`Repository::push_remotes`): the current branch's push remote, `remote.pushDefault` when `HEAD` is on no branch, the only remote, or `origin` - or one named remote, or every one, each said apart. A tag of that name already on the remote is not replaced. Deleting a tag can take it away from remotes too: a remote that keeps it gives it back on the next fetch.
- **Every typed name comes after `--`**, so a name that looks like a flag reaches git as a name and is refused as one.
- **In the window, branches and tags change one at a time with staging,** and not while a release runs - it commits, pushes and tags on the branch it started on. A push or a remote deletion of a tag goes one at a time with the other runs on a remote.

## Consequences

**Positive.** A checkout, a new branch and a deletion do the same thing in every door and on every machine, whatever its git is configured to guess. A first push of a branch made from a remote one goes to a branch of its own name. Nothing is deleted that something else does not keep, unless the person was told how much and said yes.

**Negative.** `branch.autoSetupMerge` has no say over a branch made through furca; a person who wants a branch to follow another name sets it afterwards (`furca branch follow`). Counting what a branch alone holds walks the history from every other ref, which on a very large repository is a moment's wait before the question.
