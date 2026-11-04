# 0014 — The status is kept by the paths a watcher names

Date: 2026-11-04
Status: Accepted.

## Context

The window shows the working copy as it changes, under a budget of 50 ms from a save to the status in a repository of 10 000 files. A whole status looks at every tracked file and walks the tree: 51 ms on the machine the numbers come from, before anything is read for the row. And a list of files is not yet what the window shows: each row says what the file holds - a binary, a git LFS pointer - and how many lines moved, which means opening it, and a formatter run across a repository changes thousands of files.

`git status` refreshes the timestamps in the index as it goes, which is a write, and a write to the index is what a watcher on the git directory listens for.

## Decision

**A status is a list first and what is in it after, and it is kept up to date by the paths something says changed.**

- **Two steps, one type.** `Repository::status` lists staged, unstaged, untracked and conflicted files without opening one, as the `FileChange` a commit's changes are made of. A side not read yet says so, `Content::Unread`, and a side only the working tree has has no id. `StatusCache::read_files` then reads them, a few at a time, by the same `compare` a commit's files go through. A second record type for "a file in the status" would have been a second truth about a file, and everything that draws or diffs one would have had to know both.
- **A cache by the index.** `StatusCache` holds the status with what it was read against: the commit `HEAD` is on and the index's checksum. While those are what they were, a change to a file can only move that file's row, so `refresh` looks at the named paths alone, as literal pathspecs; a folder stands for what is under it. A different stamp, a touched `.gitignore`, `.gitattributes` or `.gitmodules`, or more than 512 paths, and it reads the whole status.
- **What git ignores is dropped first.** A build writes thousands of files into an ignored folder. Each named path is held against the index and the excludes before anything else, so that writing costs a lookup per path and no scan.
- **The watcher is in the core, behind a feature.** `notify`, on the working tree and the git directory, with its events sorted into paths, the index, refs and "read everything" and gathered until they pause for 60 ms, or for 400 ms at most. The window and any later door want the same sorting; `watch` keeps the dependency away from libraries that only read. An event is a hint: what it names is read again, and a file system that says it lost events asks for everything.
- **Only the tab in front is watched.** A tab behind it shows nothing that needs keeping, and is read when it comes forward. The window also reads everything when it gets the focus back, because a network drive or a machine going to sleep can lose events without saying so.
- **Nothing is written.** The status leaves the index's timestamps as they are, so a file whose timestamps moved and whose content did not is compared again until git next refreshes it. ADR 0001 holds: everything that changes a repository goes through git.

## Consequences

**Positive.** A save reaches the status in about 19 ms on a 10 000-file working copy, repository opened and the file read, and the open diff follows within about 150 ms of the write, most of it the pause the events are gathered over. The list of a very long status is there at once and fills in. The differential test holds the cache to the whole read and to `git status --porcelain=v2` over random sequences of writes, moves, stages and commits, so the cache cannot quietly mean something else.

**Negative.** The cache trusts the paths it is told: one nobody named stays as it was until the next whole read, which the focus coming back makes soon enough. On Linux a recursive watch is one inotify watch per directory, ignored ones included; a repository past the system's limit is not watched, the window says so at its foot, and the list is as fresh as the last time the window got the focus. The window is given 1 000 rows of each list, and says how many more there are.
