# 0023 — An operation halfway is a state, and a conflict is read from its stages

Date: 2026-11-20
Status: Accepted.

## Context

v0.16 merges, cherry-picks and reverts, continues, skips and aborts what they leave halfway, and resolves conflicts - in a window's three panes, and from the CLI, which an assistant drives.

git answers each of these with a command that fails when it stops at a conflict: `git merge` exits 1 and leaves `MERGE_HEAD`, the same as when it refuses outright and leaves nothing. `--continue` opens an editor for a message, which an app's git has no one to close. The markers git writes into a conflicted file depend on `merge.conflictStyle` - the default leaves the common ancestor out - and are gone from a file someone has started to edit, so a resolver that reads them sees something different in every repository. `git merge feature` names the merge after whatever ref git finds first for `feature`: a tag of that name stands in for the branch. Picking or reverting a merge commit needs a mainline, and picking a commit `HEAD` already has stops halfway with an empty commit.

## Decision

**A stop is an answer; only a refusal is an error.** Every step - merge, cherry-pick, revert, continue, skip, abort - returns where it left the repository (`Stepped`): the operation still halfway, git's words if it stopped, the paths in conflict, `HEAD` before and after, and how many commits it made. A run that failed but left an operation halfway that was not there before, moved `HEAD`, or changed what is in conflict stopped; one that moved nothing was refused, and is an `Error` with git's words. The CLI exits 1 for a stop and 2 for a refusal.

**What is halfway is a state every door reads** (`Repository::halfway`): the operation, the branch it lands on, the commit it brings in (`MERGE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `REBASE_HEAD`), the first line of the message git wrote, how many paths are in conflict, and whether it can be continued or skipped. `furca continue`, `furca abort` and the window's bar act on that state, whoever began it - a pull that rebased and stopped included - so the operation is never named twice.

**The core says no before git does.** A second operation does not begin while one is halfway (`Error::Halfway`); nothing continues while a path is in conflict (`Error::Unresolved`); a merge has nothing to skip; a bisect can only be aborted; a pick of a commit `HEAD` already has is refused.

**git writes no message of its own choosing in an editor.** `GIT_EDITOR=:` - git's own word for none, on every system - joins `GIT_MERGE_AUTOEDIT=no` in every run: a continue takes the message git prepared. A person who wants another one commits it from the window's form or with `git commit`, which finishes the operation as well.

**A name to merge is resolved as a checkout resolves it** (`Repository::merge_target`): a full ref name as given, else a local branch, a remote-tracking branch, a tag - and else any commit. git is given the short name when its own lookup (`refs/`, tags, heads, remotes) finds that same ref, so the message reads `Merge branch 'feature'`; the full name otherwise. The window passes the full name of what was clicked.

**A merge commit is picked and reverted against its first parent** (`--mainline 1`): what it brought into the line it was merged into. Commits are resolved to ids before git runs.

**A conflict is read from the index's stages, not from the markers** (`Repository::conflict`). The ancestor, ours and theirs - stages 1, 2 and 3 - are read through the repository's attributes, as a commit's change is (a binary, an LFS pointer, a file too large to compare say so), and their lines are cut into blocks the way `diff3` cuts them, from each side's diff against the ancestor with the core's own line diff: unchanged, changed by ours, by theirs, the same on both, or a conflict. Changes that only touch are one conflict, as in git. A conflict's common head and tail lines become blocks of their own; where the ancestor had no lines there - a file both added - the two sides are compared line by line. A file one side deleted, or not text, has no blocks: it is taken whole. The file's markers are read only to say whether the working tree still holds them (`marked`).

**A resolution is written by git** (`Git::resolve`): a text becomes a blob with `git hash-object -w --no-filters`, goes into the index at stage 0 with `git update-index --index-info` - which takes the stages away - and into the working tree with `git checkout-index --force`, through the smudge filters and line-ending conversion. A text is given our side's `\r\n` back when that file had it. A whole side is its stage's blob, written the same way; no file is `git rm`. A file taken as the working tree has it is `git add --all`, refused while it holds git's markers - seven `<` closed by seven `>`; a line of `=======` alone underlines a Markdown heading as often - unless forced.

**The window resolves in three panes.** Ours and theirs above, kept level by CodeMirror's merge view with the core's blocks as its chunks; the result below, an editor, which starts with every block that goes in on its own and each conflict written out between git's markers in the diff3 style. A conflict is resolved when no markers are left in it, and the file can be saved when none are left at all: the markers in the result are the one truth of what is still to do, whether a bar's button or a hand removed them.

## Consequences

**Positive.** A merge, a pick and a revert behave the same in every door and every repository, whatever its conflict style; a stop is shown as what to do next, not as a failure. An assistant reads conflicts as blocks in JSON and resolves them with one command per file. A resolution goes through the repository's filters and keeps its line endings, and nothing that was resolved by hand elsewhere is taken without a word.

**Negative.** The blocks are worked out by furca's line diff, not git's merge: where git's merge would have drawn a conflict's edges a line differently, or a merge driver resolved a file its own way, the resolver shows the stages' raw three-way merge. A message git refuses through a hook is changed outside the bar. A rebase's steps are continued here but not edited: an interactive rebase stays in a terminal.
