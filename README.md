<p align="center"><img src="https://raw.githubusercontent.com/lacodda/furca/main/assets/banner.svg" alt="furca" width="720"></p>

> A fast git client that stays out of your way: reads in-process through gitoxide, mutations through your own git - so hooks, credentials and LFS behave exactly as they do in a terminal.

<p align="center">
  <a href="https://github.com/lacodda/furca/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacodda/furca/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://crates.io/crates/furca"><img src="https://img.shields.io/crates/v/furca?style=flat-square" alt="crates.io"></a>
  <a href="https://lacodda.github.io/furca/"><img src="https://img.shields.io/badge/docs-lacodda.github.io-blue?style=flat-square" alt="Docs"></a>
  <a href="https://github.com/lacodda/furca/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacodda/furca?style=flat-square" alt="License"></a>
</p>

## Why

Most git clients either reimplement git badly (breaking hooks, credential helpers, LFS, signing) or shell out to `git` for everything (paying a process-spawn cost on every read). Furca does neither: reads run through [gitoxide](https://github.com/GitoxideLabs/gitoxide) in-process, while mutations go through the system `git` CLI, so your config, hooks and credentials work exactly as they do in a terminal. See [ADR 0001](https://github.com/lacodda/furca/blob/main/docs/adr/0001-hybrid-git-engine.md).

## In a terminal

```console
$ furca status
On main at 171154d, tracking origin/main

$ furca log --all -n 3
171154d 2026-09-17 Ada Author  docs: fill the readme out as a shopfront
d402e85 2026-09-17 Ada Author  chore: point components.json at the dowel registry
c8292c5 2026-09-04 Ada Author  build: raise the MSRV to 1.88
... more history; raise --limit (now 3)

$ furca refs --json | jq '.branches[0]'
{ "name": "main", "target": "171154d…", "upstream": "origin/main", "head": true }
```

- **Parents after children, always.** `furca log` orders by the graph and uses time only to break ties, so a machine with a slow clock cannot put a parent above its child - the list draws as a graph top to bottom.
- **JSON for scripts and assistants.** Every command takes `--json`; the output is the engine's own types, not a second format kept in step by hand.
- **One engine, three doors.** `furca-core` is a plain Rust library; the CLI, the desktop window and - later - an MCP server are thin wrappers around it. See [ADR 0002](https://github.com/lacodda/furca/blob/main/docs/adr/0002-one-core-three-doors.md).

## Install

```powershell
irm https://raw.githubusercontent.com/lacodda/furca/main/tools/install.ps1 | iex   # Windows
```

```bash
curl -fsSL https://raw.githubusercontent.com/lacodda/furca/main/tools/install.sh | sh   # macOS, Linux
cargo install furca                                                                   # anywhere with Rust
```

## Status

**v0.1.0** is the engine and the CLI: `status`, `refs` and `log`, with `furca-core` published as a library. The desktop window and the working-tree status come in later releases - see the [CHANGELOG](https://github.com/lacodda/furca/blob/main/CHANGELOG.md).

## Documentation

Full documentation: [lacodda.github.io/furca](https://lacodda.github.io/furca/). Architecture decisions live in [docs/adr](https://github.com/lacodda/furca/tree/main/docs/adr).

## License

MIT (c) [Kirill Lakhtachev](https://lacodda.com)
