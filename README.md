<p align="center"><img src="https://raw.githubusercontent.com/lacodda/furca/main/assets/banner.svg" alt="furca" width="720"></p>

> A fast git client that stays out of your way: reads in-process through gitoxide, mutations through your own git - so hooks, credentials and LFS behave exactly as they do in a terminal.

<p align="center">
  <a href="https://github.com/lacodda/furca/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacodda/furca/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://github.com/lacodda/furca/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacodda/furca?style=flat-square" alt="License"></a>
</p>

## Why

Most git clients either reimplement git badly (breaking hooks, credential helpers, LFS, signing) or shell out to `git` for everything (paying a process-spawn cost on every read). Furca does neither: reads — log, status, diff, blame, refs — run through [gitoxide](https://github.com/GitoxideLabs/gitoxide) in-process, while mutations — commit, merge, rebase, push, pull, stash — go through the system `git` CLI, so your config, hooks and credentials work exactly as they do in a terminal. See [ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md).

## What you get

- **One engine, three doors.** `furca-core` holds the logic; a Tauri desktop app, a headless CLI, and — eventually — an MCP server are thin wrappers around it with no logic of their own. See [ADR 0002](https://github.com/lacodda/furca/blob/main/docs/adr/0002-one-core-three-doors.md).
- **A hybrid engine, not a reimplementation.** Reads go through gitoxide in-process; every mutation shells out to your own `git`, so hooks, credential helpers, LFS and signing behave exactly as they do in a terminal.
- **A scriptable core.** `furca status` prints the repository's `HEAD` — branch, commit, detached or not — as JSON, discovering the repository the way `git` itself would, by walking up through parent directories.
- **A release engine, built in.** furca will drive its own versioning from a terminal rather than depending on an external release tool. See [ADR 0003](https://github.com/lacodda/furca/blob/main/docs/adr/0003-release-engine-inside-the-client.md).

## Install

Not published as a binary yet. Builds for Windows, macOS and Linux, plus the `furca` CLI, will appear on the [Releases page](https://github.com/lacodda/furca/releases) with the first tagged version.

Until then, build from source — see [CONTRIBUTING.md](https://github.com/lacodda/furca/blob/main/CONTRIBUTING.md).

## Status

**Pre-release**, nothing tagged yet. What exists today: the repository scaffold, the engine's read path (`furca-core`, wrapping gitoxide), the `furca status` CLI command, and a desktop shell that proves the door from the window to the engine is wired end to end — by its own admission in the source, "scaffolding, not the product." The release engine and the rest of the client are not built.

## Documentation

Full documentation: [lacodda.github.io/furca](https://lacodda.github.io/furca/). Architecture decisions live in [docs/adr](https://github.com/lacodda/furca/tree/main/docs/adr).

## License

MIT (c) [Kirill Lakhtachev](https://lacodda.com)
