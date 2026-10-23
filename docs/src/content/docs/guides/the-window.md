---
title: The window
description: Open repositories in tabs, save sets of them as sessions, read their history as a graph, search it, and see their branches, remotes, tags, stashes and submodules.
---

The desktop window is the third door onto the same engine as the CLI: what it
shows of a repository is what `furca log`, `furca refs` and `furca line` print,
read the same way, in-process. It is laid out as Fork lays a git client out -
tabs of repositories along the top, a column of refs on the left, the graph of
the history beside it - and it draws its own title bar, so the tabs sit in it
rather than in a band under it.

![The window with three repositories open: the branches in folders on the left, the graph of the history with its branches and tags beside it](../../../assets/window.png)

## Get it

There is no installer yet: the window's installers wait for code signing, since
an unsigned one is what SmartScreen and Gatekeeper turn away. CI builds them for
every platform on every push, and the window builds from a checkout:

```sh
pnpm install
pnpm tauri build          # or `pnpm tauri dev` to run it from the source
```

See [CONTRIBUTING](https://github.com/lacodda/furca/blob/main/CONTRIBUTING.md)
for what it needs.

## Opening repositories

Each open repository is a tab. A repository opens

- from **Open repository…** (<kbd>Ctrl</kbd>+<kbd>O</kbd>), which asks for a
  folder;
- from **Recent**, the repositories opened lately, in the `+` menu after the
  tabs and on the empty window;
- by **dropping a folder** on the window - or a file: a path inside a working
  tree opens the repository around it, as `git -C` does;
- from a terminal: `furca-desktop PATH` opens `PATH`. With the window open
  already, the path opens as a tab in it and the window comes forward - there
  is one window, and its tabs are the one list of what is open.

Opening a repository that is open already brings its tab forward instead of
opening a second one, however the path was written. Two tabs with the same
directory name add their parent's, so the bar never shows two it cannot tell
apart.

| Keys | |
| --- | --- |
| <kbd>Ctrl</kbd>+<kbd>O</kbd> | Open a repository |
| <kbd>Ctrl</kbd>+<kbd>W</kbd>, <kbd>Delete</kbd> on a tab, a middle click | Close the tab |
| <kbd>Ctrl</kbd>+<kbd>Tab</kbd>, <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> | The next or the previous tab |
| <kbd>Ctrl</kbd>+<kbd>1</kbd> … <kbd>Ctrl</kbd>+<kbd>8</kbd>, <kbd>Ctrl</kbd>+<kbd>9</kbd> | That tab, the last tab |

Tabs are dragged along the bar to change their order; a strip too long for the
bar scrolls with the wheel. The keys are read by their place on the keyboard,
so they work on any layout.

## Sessions

A session is a set of tabs saved under a name: **Save tabs as a session…** in
the `+` menu. Opening it later replaces the open tabs with its own, in its
order, with the one that was in front in front again. Saving under a name
that is taken replaces that session, and the dialog says so first.

**The line** is a session that is never saved: every repository your
[rigger](https://lacodda.github.io/rigger/) record keeps - the list
[`furca line`](/furca/reference/line/) reads - fetched from the record each time
it opens, so a project added to the record is in it the next time. It is
offered only where rigger is installed.

A repository of a session that is no longer there is left out, and the window
says which.

## The left column

| Section | What it shows |
| --- | --- |
| **Branches** | Local branches, in folders by the `/` in their names. The branch `HEAD` is on is in bold with its mark in the accent; `↑2 ↓1` is how far it is from its upstream, and `gone` an upstream deleted on the remote and pruned. |
| **Remotes** | Each remote with its remote-tracking branches, in folders the same way. Pointing at a remote shows its address, without any password written into it. |
| **Tags** | Newest version first: `v0.10.0` comes before `v0.9.0`. |
| **Stashes** | The stash, newest first, by the message git wrote. |
| **Submodules** | Each submodule by its path, marked `not checked out` when it is not, and `moved` when what is checked out is not the commit the repository records. |

The distances are against the last fetch: the window reads this machine only
and asks no remote. The column is one control - <kbd>Tab</kbd> reaches it once,
the arrows move inside it, <kbd>Right</kbd> and <kbd>Left</kbd> open and close a
folder. Choosing a branch, a remote branch, a tag or a stash entry takes the
graph to its commit.

## The graph

Beside the column is the history, in the order `git log --all --date-order`
lists it: a commit is never below one of its parents, and among the commits
that could come next the newest goes first. Each line of descent is a lane of
its own colour, and a branch keeps its lane from its tip down to where it forked
off, so the eye can follow it. A hollow dot is a merge; the commit `HEAD` is on
has a ring.

| Column | |
| --- | --- |
| **Graph** | The lanes. A history wider than sixteen lanes is cut at the right edge of the column. |
| **Subject** | The first line of the message, after what points at the commit: the branch `HEAD` is on in the accent, other branches, remote-tracking branches, tags and stash entries. |
| **Author** | Who wrote it; pointing at the name shows the address. |
| **Date** | When it was written, in your language's format. |
| **Commit** | The short id. |

Above the graph, the branch `HEAD` is on and how far it stands from its
upstream, with what is changed, untracked and stashed. **History of** shows
the history of one branch - local or remote-tracking - instead of everything;
the commit you had chosen stays chosen when it is in that history too.

The graph is read in pages as it scrolls, and the history is laid out in the
background while the first screen is already there: a history of a hundred
thousand commits opens as fast as one of a hundred. See
[ADR 0012](https://github.com/lacodda/furca/blob/main/docs/adr/0012-the-graph-stays-in-the-app-and-the-window-asks-for-rows.md).

### Searching

<kbd>Ctrl</kbd>+<kbd>F</kbd> searches the history: the words of a subject, an
author's name or address, or the start of a commit's id - four hex digits or
more, as git takes an abbreviated id. Case does not matter. The search goes
through the whole history, the first matches arrive at once, and the window
moves to the first one from where you are; the places found are marked in each
row.

| Keys | |
| --- | --- |
| <kbd>Enter</kbd>, <kbd>Shift</kbd>+<kbd>Enter</kbd> | The next or the previous match |
| <kbd>Escape</kbd> | End the search |

With [nooma](https://lacodda.github.io/nooma/) on the `PATH`, the search asks
its history of the repository too, which reads the whole message: a commit
whose reason is written only in the body of its message is found, and marked
**in the message**. nooma reads a repository's history the first time it is
asked and keeps it, so the first search there takes a moment longer. Without
nooma the search is furca's own.

### Moving through the history

The graph is one control: <kbd>Tab</kbd> reaches it once.

| Keys | |
| --- | --- |
| <kbd>Up</kbd>, <kbd>Down</kbd> | The commit above or below |
| <kbd>Page Up</kbd>, <kbd>Page Down</kbd> | A screen up or down |
| <kbd>Home</kbd>, <kbd>End</kbd> | The newest commit, the first one |

The window reads a tab again when it comes forward and when the window gets
the focus back, so a commit made in a terminal meanwhile shows up; the graph is
laid out again only when a branch, a tag or `HEAD` moved.

## What the window writes

Nothing in your history. One file, in the background: when a repository has no
commit-graph file, or one more than a thousand commits behind, the window has
git write one - `git commit-graph write --reachable --split`, the file `git gc`
writes on its own - and the status line along the foot says so. Without it the
graph of a long history costs seconds to open; with it, milliseconds. A
repository with `core.commitGraph` set to `false` is left alone. See
[ADR 0010](https://github.com/lacodda/furca/blob/main/docs/adr/0010-the-window-has-git-write-the-commit-graph.md).

What the window remembers - the open tabs, the recent list, the sessions - is
one file, `state.json`, in the app's data directory: `%APPDATA%\com.lacodda.furca`
on Windows, `~/Library/Application Support/com.lacodda.furca` on macOS and
`~/.local/share/com.lacodda.furca` on Linux. A file that cannot be read is moved
aside as `state.broken.json` and the window starts empty.

`FURCA_DESKTOP_DATA` names another directory for all of it, the webview's own
profile included - for a run that must not touch the window you use every day.
Such a run is a window of its own: it neither hands its paths to the window you
use nor takes theirs.

## Speed

The window holds budgets of its own, measured on the built app before every
commit and with every release: it is on the screen within 300 ms of its start,
in the colours of your theme, and the app and its webview hold under 300 MB
with three repositories of 100 000 commits open, each graph laid out to its
end - on Windows; Linux's number, measured without a GPU, is published but
not promised. The first frame with a repository - its column and the first rows of its
graph - is measured and published too. See [the budgets](/furca/concepts/budgets/).
