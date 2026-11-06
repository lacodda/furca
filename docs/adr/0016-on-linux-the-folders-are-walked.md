# 0016 — On Linux the folders are walked

Date: 2026-11-06
Status: Accepted. Changes one consequence of ADR 0014.

## Context

ADR 0014 watches the working tree whole, and accepts that on Linux a recursive watch is one inotify watch per folder, ignored ones included. A `target/` and a `node_modules/` are most of the folders of a repository and none of its working copy, and they are what reach the system's limit. Past it the window says the tree is not watched and is only as fresh as the last time it got the focus: the budget for a save means nothing there. Windows and macOS watch a tree natively, whatever its size.

## Decision

**On Linux, each folder worth hearing gets a watch of its own; the rest get none.**

- **What is walked.** Every folder of the working tree except what git ignores, held against the same excludes the status uses. A folder with something tracked under it stays however it is ignored, because git ignores nothing it tracks. Of a git directory, the directory itself and `refs`, `info`, `rebase-merge` and `rebase-apply`: nothing else there can say anything the watcher listens for, and `objects` alone is 256 folders.
- **It keeps up by walking again, before a batch goes out.** A folder appearing, moving or going, a `.gitignore` written, the configuration changed or events lost all mean the folders are walked again once the events pause. That is what closes the gap a new folder opens: what was written in it before its watch was there is read with the batch that names the folder, and what is written after is heard.
- **A folder that moved or went is let go at once.** An inotify watch follows the inode rather than the name, and one of the same name made in its place would go unheard.
- **The other systems stay as they are.** A walk is more work than a recursive watch, and they have nothing to gain from it. The walked way runs on every system in the tests, so a Windows machine can check it; only a rename is left to Linux, because Windows will not move a folder a handle is held on.
- **A watch that cannot be made says so.** At the start the tab says it is not watched, as before. Later, `Batch::unwatched` carries the reason and the foot of the window says the same.

## Consequences

**Positive.** A repository's watches are about as many as the folders of its working copy. A build in an ignored folder costs no kernel watches and no events, on top of no reads.

**Negative.** Each batch that follows a new folder walks the working tree again, which is a read of every folder that is not ignored. A file written into an ignored folder that is not tracked goes unheard, which is the point; a folder ignored and then tracked with `git add -f` is picked up at the next walk, which the index changing does not start by itself. The whole read when the window gets the focus covers that case, as it covers every other event a file system loses.
