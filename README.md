<p align="center"><img src="https://raw.githubusercontent.com/lacodda/furca/main/assets/banner.svg" alt="furca" width="720"></p>

> A fast git client that stays out of your way: reads in-process through gitoxide, mutations through your own git - so hooks, credentials and LFS behave exactly as they do in a terminal.

<p align="center">
  <a href="https://github.com/lacodda/furca/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacodda/furca/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://crates.io/crates/furca-core"><img src="https://img.shields.io/crates/v/furca-core?style=flat-square&label=furca-core" alt="furca-core on crates.io"></a>
  <a href="https://lacodda.github.io/furca/"><img src="https://img.shields.io/badge/docs-lacodda.github.io-blue?style=flat-square" alt="Docs"></a>
  <a href="https://github.com/lacodda/furca/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacodda/furca?style=flat-square" alt="License"></a>
</p>

## Why

Most git clients either reimplement git badly (breaking hooks, credential helpers, LFS, signing) or shell out to `git` for everything (paying a process-spawn cost on every read). Furca does neither: reads run through [gitoxide](https://github.com/GitoxideLabs/gitoxide) in-process, while mutations go through the system `git` CLI, so your config, hooks and credentials work exactly as they do in a terminal. See [ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md).

## In a window

<p align="center"><img src="https://raw.githubusercontent.com/lacodda/furca/main/assets/window.png" alt="furca's window: repositories in tabs, branches in folders on the left, the graph of the history, and under it the chosen commit - its files, a binary and a git LFS file named with their sizes, and a change side by side" width="860"></p>

## In a terminal

```console
$ furca status
On main at 171154d, tracking origin/main

$ furca log --all -n 3
171154d 2026-09-17 Ada Author  docs: fill the readme out as a shopfront
d402e85 2026-09-17 Ada Author  chore: point components.json at the dowel registry
c8292c5 2026-09-04 Ada Author  build: raise the MSRV to 1.88
... more history; raise --limit (now 3)

$ furca refs --json | jq '.branches[0].upstream'
{ "name": "origin/main", "ahead": 1, "behind": 0 }
```

<!-- The tags below are other repositories' own: the release bump leaves them. -->
<!-- historical versions -->
```console
$ furca line "$HOME/src/*"
REPOSITORY  BRANCH  UPSTREAM    TAG     TAGGED      SINCE  ONLY HERE
api         main    up to date  v1.4.0  2026-10-02      0  -
notes       main    3 ahead     v0.2.1  2026-09-28      5  2 changed, 1 untracked, 3 unpushed

2 repositories: 1 settled, 1 with work only here.
```
<!-- /historical versions -->

- **Fast, and it says so.** Opening a 100 000-commit repository and reading the first 500 rows of its graph takes about 25 ms against a budget of 100 ms. The budgets are part of the gate, and every release publishes what it measured on [the budgets page](https://lacodda.github.io/furca/concepts/budgets/).
- **Parents after children, always.** `furca log` orders by the graph and uses time only to break ties, so a machine with a slow clock cannot put a parent above its child - the list draws as a graph top to bottom.
- **Every repository at a glance.** `furca line` reads a whole set of repositories at once - the ones your [rigger](https://lacodda.github.io/rigger/) record keeps, or a pattern - and says what each holds that is nowhere else yet: uncommitted, unpushed, stashed, and what has not been released since the last tag. See [line](https://lacodda.github.io/furca/reference/line/).
- **JSON for scripts and assistants.** Every command takes `--json`; the output is the engine's own types, not a second format kept in step by hand.
- **A release that runs itself.** `furca release plan` proposes the next free version and checks every shop window before a tag; `furca release run` carries it out - bump, changelog, gate, commit, push, CI, tag, publish - then verifies the release page, registries and links, installs the release and proves the installed binary is its own build. It stops at the first red step; `resume` picks up after the fix. See [release](https://lacodda.github.io/furca/reference/release/).
- **What a commit changed, binaries named.** `furca show` lists a commit's files with their lines, and says which are binaries or git LFS files and how large - before anyone opens a diff. `furca diff` prints the lines as `git diff` does. See [show](https://lacodda.github.io/furca/reference/show/).
- **Repositories in tabs, history as a graph.** The window opens a folder, a session, or every repository your rigger record keeps. The graph is drawn on a canvas a screen at a time, so 100 000 commits open like 100; under it, the chosen commit's files and a file's change, side by side or inline. See [the window](https://lacodda.github.io/furca/guides/the-window/).
- **One engine, three doors.** `furca-core` is a plain Rust library; the CLI, the desktop window and - later - an MCP server are thin wrappers around it. See [ADR 0002](https://github.com/lacodda/furca/blob/main/docs/adr/0002-one-core-three-doors.md).

## Install

```powershell
irm https://raw.githubusercontent.com/lacodda/furca/main/tools/install.ps1 | iex   # Windows
```

```bash
curl -fsSL https://raw.githubusercontent.com/lacodda/furca/main/tools/install.sh | sh   # macOS, Linux
```

## Status

**v0.8.0** reads a repository whole and writes only releases. The CLI and the release engine, which released this version itself, install from the release; the window shows the history and a commit's change, and builds from a checkout until its installers are signed. The working tree comes next - see the [CHANGELOG](https://github.com/lacodda/furca/blob/main/CHANGELOG.md).

## Documentation

Full documentation: [lacodda.github.io/furca](https://lacodda.github.io/furca/). Architecture decisions live in [docs/adr](https://github.com/lacodda/furca/tree/main/docs/adr).

## License

MIT (c) [Kirill Lakhtachev](https://lacodda.com)
