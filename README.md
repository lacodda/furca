<p align="center"><img src="https://raw.githubusercontent.com/lacodda/furca/main/assets/banner.svg" alt="furca" width="720"></p>

# Furca

[![CI](https://github.com/lacodda/furca/actions/workflows/ci.yml/badge.svg)](https://github.com/lacodda/furca/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/furca.svg)](https://crates.io/crates/furca)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/lacodda/furca/blob/main/LICENSE)

**A fast git client that stays out of your way.** Reads happen in-process through gitoxide, mutations go through your own git, so hooks, credentials and LFS behave exactly like the terminal. A headless CLI and a release engine are built in.

## Why

Most git clients either reimplement git badly (breaking hooks, credential helpers, LFS, signing) or shell out to `git` for everything (paying a process-spawn cost on every read). Furca does neither: reads — log, status, diff, blame, refs — run through [gitoxide](https://github.com/GitoxideLabs/gitoxide) in-process, while mutations — commit, merge, rebase, push, pull, stash — go through the system `git` CLI, so your config, hooks and credentials work exactly as they do in a terminal. See [ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md).

The same engine is exposed three ways: a Tauri desktop app, a headless CLI (`furca status`, JSON output), and a release engine that drives its own versioning from a terminal. See [ADR 0002](https://github.com/lacodda/furca/blob/main/docs/adr/0002-one-core-three-doors.md) and [ADR 0003](https://github.com/lacodda/furca/blob/main/docs/adr/0003-release-engine-inside-the-client.md).

## Status

**Pre-release.** v0.1.0 is in progress: the repository scaffold, the engine's read path, and the desktop shell exist; the release engine and the rest of the client are not built yet.

## Install

Coming with the first release. Builds for Windows, macOS and Linux, plus the `furca` CLI binary, will be published on the [Releases page](https://github.com/lacodda/furca/releases).

## Development

Requires Rust, Node 22+ and pnpm.

```sh
pnpm install
pnpm tauri dev            # run the desktop app
cargo test                # engine and CLI tests
```

Full documentation: [lacodda.github.io/furca](https://lacodda.github.io/furca/). Architecture decisions live in [docs/adr](https://github.com/lacodda/furca/tree/main/docs/adr).

## License

MIT
