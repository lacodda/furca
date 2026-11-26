# 0024 — A rebase is given its plan, and a rewrite in place happens whole

Date: 2026-11-26
Status: Accepted.

## Context

v0.17 rebases: the current branch onto another, and by a plan that reorders, rewords, folds and drops its commits. It also squashes the last commits into one, and absorbs a change into a commit already made - the fixup-and-autosquash a person otherwise does in three commands.

ADR 0023 left an interactive rebase in a terminal, because git asks for it there: `git rebase -i` writes a todo list and opens an editor on it, `reword` and `squash` open another for each message, and every run of git here is given `GIT_EDITOR=:` so that nothing waits for a person. An app has to hand git the finished plan instead.

A todo list is also easy to get wrong in a way git accepts. A line deleted is a commit dropped, without a word; a `fixup` on the first line folds into nothing and fails halfway. An assistant that writes the list from a log, and a window that keeps it as rows, can both lose a row.

A rebase replays a merge's side as commits of the branch itself, unless it is told to rebuild merges - and a plan that rebuilds merges is a script of labels and resets, not a list of commits to drag.

git's autostash, which carries uncommitted changes over a rebase, brings them back unstaged: what was staged before is in the working tree after, and the index has forgotten it.

## Decision

**The plan is read before it is written** (`Repository::rebase_plan`). The commits a rebase onto a target would replay - `target..HEAD`, or every commit from the first for no target - come oldest first, each with its whole message and whether the branch's upstream has it already. A merge among them is refused: the plan is a line of commits, and a rebase that would flatten a merge is not begun here. A target is looked for as a checkout looks for one - a full ref name, a local branch, a remote-tracking branch, a tag, any commit - and given to git as a commit id.

**A todo list names every commit of the plan once** (`Git::rebase_interactive`). Each entry is a commit and what becomes of it - `pick`, `reword` with its message, `squash`, `fixup`, `drop` - in the order to replay them. A commit of the plan the list leaves out is refused, not dropped; so is one named twice, one that is not in the plan, a reword without a message, and a `squash` or `fixup` with no commit replayed before it. The core checks all of it before git runs, so a list that is wrong moves nothing.

**git is handed the plan by a sequence editor that is one line of shell.** The todo list is written to `furca-rebase/todo` in the git directory, and `GIT_SEQUENCE_EDITOR` is a shell function that copies it over the file git names, in words every `sh` has built in (`read`, `printf`): git runs its editors through its own `sh` on every system, Windows included, so no helper program is shipped and none is looked for. A reworded commit is a `pick` followed by `exec git commit --amend --file=…`, its message in a file beside the plan: the hooks judge it as they judge any commit, and one a hook refuses stops the rebase there. A `squash` takes the two messages one after the other, as git prepares them; the editor it would open is `:`. The folder is taken away by the first step that finds no rebase halfway.

**A rebase begun here carries uncommitted changes with it** (`--autostash`), whatever `rebase.autoStash` says: a window whose working copy is rarely clean would otherwise be refused each time, and one way is better than a checkbox. A commit that comes out empty is dropped (`--empty=drop`), as a plain rebase drops it.

**A rebase that stops is an answer** (ADR 0023): `Stepped` says so, and `continue`, `skip`, `abort`, `conflicts` and `resolve` take it from there. `Halfway` now says how far a rebase has got - the step it stopped at and how many there are - and gives the message git prepared whole, which the window's commit form starts from when nothing is typed in it.

**A squash is not a rebase** (`Git::squash`). The commits from one named up to `HEAD` become one by moving the branch back to it with the index and the working tree left alone (`reset --soft`) and amending it: the new commit holds exactly what `HEAD` held, so nothing can conflict, the author stays the first commit's, and the hooks run. Its message is the one given, or the commits' messages one after another. It is refused while something is staged - that would go into the commit unasked - and a hook that refuses the message leaves the branch where it was.

**An absorb happens whole or not at all** (`Git::absorb`, `Git::absorb_lines`). What is staged - or the lines named, of a file that is not staged - is made a commit on `HEAD` whose message is `fixup! <the target's id>`, by id so that two commits with one subject cannot be mistaken; `git rebase --autosquash` then replays it straight after its target, as one commit with it. The tree of that commit is written from the index - for lines, from an index of its own that holds `HEAD` and those lines, so what else is staged stays out. If the replay stops - the change does not fit the target, or a later commit does not replay over it - the rebase is aborted, the branch and the index are put back exactly, and the refusal says so: a change that cannot be absorbed is not left as a rebase to finish. On either way out the index is read back from the tree it held (`read-tree`), which is what keeps staged changes staged across git's autostash.

**Only a line of commits is rewritten in place.** A squash and an absorb take a commit of the line `HEAD` came by, with no merge from it on; the window offers those and no others (`Repository::rewritable`).

## Consequences

**Positive.** An interactive rebase is a list in JSON for an assistant and rows to drag for a person, and both are held to the same plan: no commit is lost to a line left out. Nothing opens an editor on any system, and nothing but git and its own shell is needed. A squash cannot conflict. An absorb never leaves a repository halfway, and keeps the index as it was.

**Negative.** History with merges in it is not rewritten here: a rebase by a plan, a squash and an absorb all stop at the first merge, and a person who wants to rebuild merges uses `git rebase --rebase-merges` in a terminal. A plain rebase onto a branch still flattens merges as git does by default, since it follows the repository's own configuration. A reworded commit is amended by an `exec`, so `pre-commit` runs for it where git's own `reword` would skip that hook. An absorb that a later commit does not replay over is refused even where a person could have resolved the conflict: they rebase by a plan instead. The words of a refusal are git's, in whatever language it speaks.
