# 0012 — The graph stays in the app, and the window asks for rows

Date: 2026-10-23
Status: Accepted.

## Context

The window draws the history of each open repository as a graph: a lane per line of descent, a row per commit, the subject, author, date and id beside it. A history can be a hundred thousand commits, and the window keeps several repositories open at once under a budget of 300 MB for three of them. On the machine the numbers come from, the window with three repositories read held about 150 MB before the graph, and on Linux CI, where the webview renders in software, 298.

The research behind furca asked for the layout to be incremental and sent to the window in chunks. Sent whole, a history of 100 000 commits is tens of megabytes of objects in the webview per tab - each commit's id, subject, author and dates, and the lines of its row - for a view that shows forty rows at a time.

## Decision

**The graph lives in furca-desktop, one per open tab, and the webview asks for the rows it shows.**

- **The core lays it out a chunk at a time.** `furca_core::Graph` holds the walk (ADR 0005) and the state of the lanes, owns no repository, and is grown by `Graph::extend(repo, rows)` from whichever thread asks next. The tab's graph is begun when the tab asks for it, its first 500 rows laid out at once, and the rest on a thread of its own in chunks of 10 000, after each of which the window hears an event with how far it has come.
- **A row keeps only what its commit changes.** Its column, its colour, the lanes that end at it and the lanes it leads down into. Most of a wide graph is lanes passing by, and those follow from the lanes above the row: the state of the lanes is kept every 64 rows, and a window of rows is drawn by playing the changes forward from the mark before it. Memory follows the number of commits, not commits times the width of the graph.
- **The window reads pages.** A page is 100 rows with their commits, read from the repository when it comes into view; forty pages are kept per tab, the least lately used let go. The graph column is one canvas the height of the view, redrawn as it scrolls; the text is a screenful of rows in the DOM.
- **A graph is asked for again, not watched.** When a tab comes forward or the window gets the focus back, the window asks for the graph again, and the app answers with the one it holds while the refs still lead where they did (`Repository::graph_is_current`), or begins a new one. A new graph has a new generation; a page asked of an older one is refused rather than answered from a history that has moved.
- **Search runs where the history is.** The subject, the author and the start of the id are searched in the app, a chunk at a time, and what is found streams to the window over a Tauri channel as it is found; a newer search stops an older one. When nooma is installed, its history of the repository (`nooma repo history --json`, a documented contract) is asked at the same time and finds words in the body of the message, which furca's own search does not read.

## Consequences

**Positive.** A tab holds the same in the webview whatever the length of its history: the webview's heap is about 5 MB with three histories of 100 000 commits open. The first screen does not wait for the history: 500 rows are laid out in about 10 ms with a commit-graph file, and the whole history of 100 000 commits in about 50. One graph per repository is the one truth the window, the search and the jump from a branch read.

**Negative.** Every page is a call across IPC and a read of a hundred commits; a fast scroll through a long history asks for pages as it goes, and a row whose page has not arrived is blank for a frame. The app holds each open tab's graph - a few megabytes per 100 000 commits, more while a graph is being laid out and its walk is alive. Search reads every commit from the object database, about 300 ms for 100 000, because the texts are not kept: keeping them would be tens of megabytes per tab for a search that is not running.
