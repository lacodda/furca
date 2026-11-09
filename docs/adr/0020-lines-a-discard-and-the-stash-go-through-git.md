# 0020 — Lines, a discard and the stash go through git

Date: 2026-11-09
Status: Accepted.

## Context

v0.13 lets the window take less than a hunk, throw a change away, and put changes in the stash and get them back. Each could be done by furca writing files itself, and each has a way to go wrong that nobody sees until something is lost: a line staged in the wrong place, a discard that wrote LF over a file a checkout gave CRLF, a stash entry dropped that was not the one shown.

## Decision

**Every one of them is a run of git, through `git_cmd`, as staging a hunk is (ADR 0019).**

- **Lines are a patch too.** `Picked` names, per hunk, which of its removed lines and which of its added lines to take; a whole hunk is every line of both (`Picked::whole`), so there is one path and no separate hunk call. The patch keeps a removed line that is left out as context and drops an added one that is left out. Lines go in by their place: the k-th removed line, then the k-th added one, so taking `2 → TWO` out of a block `2, 3 → TWO, THREE` gives `TWO, 3` and not `3, TWO`. Hunks close enough for their context to meet go in one patch hunk, as `git diff` writes them, since a second hunk whose context named lines the first had changed would not apply. A last line with no newline that a kept or an added line would follow is ended, which is the one case where the result says more than the lines picked.
- **A discard is the same patch the other way, applied to the working tree.** `git apply` without `--cached` reads the file through the clean filters and writes it back through the smudge filters and `core.autocrlf`, so a file on disk keeps its line endings. A whole file is `git restore --worktree`, which puts back what the index has and leaves what is staged. An untracked file is deleted with `git clean`, which takes paths as arguments, not on standard input, so a long list is a few runs, each under the shortest command line a system takes.
- **The stash is `git stash`.** A push always takes untracked files (`--include-untracked`): a stash that leaves a new file behind is the surprise of putting work away and finding half of it still there. Apply and pop are plain, without `--index`: the staged half comes back unstaged, but the entry applies wherever the plain one would. An entry is named by its place, `stash@{n}`, and that place is checked against the commit the window read just before the run; an entry made or dropped since moves the rest, and a stale one is refused as `Error::Moved` rather than acted on.
- **A stash entry holds its untracked files.** `Repository::changes` adds the files of a stash entry's third parent as added, so the window, `furca show stash@{0}` and `furca diff stash@{0}` say all of what it put away.
- **A failure says where.** A stash that meets a conflict says so on standard output and nothing on standard error; a run that fails in silence on standard error keeps what it said on standard output, so the console and the error name the file.
- **The window asks before anything is lost**, with dowel's confirm dialog: a click beside it does nothing.

## Consequences

**Positive.** What git refuses, the window refuses: a stale screen cannot put a line, discard a line, or drop an entry it no longer shows. Line endings and filters are git's, not furca's. The console shows every discard and stash as a command one could type.

**Negative.** Nothing keeps a discarded change: git never stored it. The question before it is the only guard; a trash for discarded changes would be furca's own store, and is not built. Picking lines by their numbers is a pointer's way; the keyboard takes whole hunks and rows.
