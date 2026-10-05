# Contributing to furca

## Development

The engine and the CLI need only Rust. The desktop shell also needs Node 22+
and pnpm.

| Crate | What it is |
| --- | --- |
| `crates/furca-core` | The git library: reads through gitoxide, no terminal or window. |
| `crates/furca-release` | The release engine: reads `release.toml`, plans the next release. |
| `crates/furca-cli` | The `furca` binary, a thin door onto both. |
| `src-tauri` | The desktop window, not published as a crate. |

The crates on crates.io build on the workspace's `rust-version`; the window
declares its own, because it follows Tauri.

```sh
cargo test --workspace                 # engine, CLI and the release gate
cargo run -p furca -- log -n 5         # the CLI against this repository
pnpm install && pnpm tauri dev         # the desktop shell
```

Before a commit, all of these are green:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p furca-core --bench budgets   # the speed budgets; red when one is broken
pnpm lint
```

The budgets are listed in `crates/furca-core/benches/budgets.json`, which
the docs site reads as well. The first benchmark run generates a 100 000-commit
fixture under `target/tmp` and takes a few minutes; later runs reuse it.

Engine tests build their repositories with `git` in a temporary directory and
never read a repository on your machine. The release engine's tests play the
registries and the record with fakes, so they need no network and no rigger.

`cargo run -p furca -- release plan` says whether this repository is ready to
release, from its own `release.toml`.
