# Contributing to furca

## Development

The engine and the CLI need only Rust. The desktop shell also needs Node 22+
and pnpm.

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
never read a repository on your machine.
