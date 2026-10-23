# Contributing to furca

## Development

The engine and the CLI need only Rust. The desktop shell also needs Node 22+
and pnpm.

| Crate | What it is |
| --- | --- |
| `crates/furca-core` | The git library: reads through gitoxide, no terminal or window. |
| `crates/furca-release` | The release engine: reads `release.toml`, plans the next release and runs it. |
| `crates/furca-cli` | The `furca` binary, a thin door onto both; installed from the release, not published as a crate. |
| `src-tauri` | The desktop window, not published as a crate. |

The crates on crates.io build on the workspace's `rust-version`; the window
declares its own, because it follows Tauri.

```sh
cargo test --workspace                 # engine, CLI, window store and the release gate
cargo run -p furca -- log -n 5         # the CLI against this repository
pnpm install && pnpm tauri dev         # the desktop window
```

`FURCA_DESKTOP_DATA=<dir>` runs the window on a data directory of its own -
its tabs, sessions and webview profile - so a run from the source never
touches the window you use.

Before a commit, all of these are green:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo bench -p furca-core --bench budgets   # the speed budgets; red when one is broken
pnpm lint
pnpm build                                  # the frontend the window's benchmark serves
cargo bench -p furca-desktop --bench window --features custom-protocol
cd docs && pnpm build
```

That is the gate (`rigger gate furca`), in its order. The window's benchmark
builds the app as `tauri build` does - its build is the release build of the
window - so the gate does not build the window a second time.

The budgets are listed in `crates/furca-core/benches/budgets.json`, which
the docs site reads as well; the core's are measured by its benchmark, the
window's by starting the built app. The first benchmark run generates a
100 000-commit fixture under `target/tmp` and takes a few minutes; later runs
reuse it. The window's benchmark needs a desktop session: it opens the window
some thirty times.

The types the window reads are generated from the Rust ones (ADR 0009) into
`src/lib/generated/`. A change to one of them fails `cargo test` until the
folder is written again:

```sh
FURCA_BLESS=1 cargo test -p furca-desktop --test bindings
```

Engine tests build their repositories with `git` in a temporary directory and
never read a repository on your machine. The release engine's tests play the
registries, the forge and the record with fakes, and push to a bare
repository next to the one they release, so they need no network and no
rigger.

`cargo run -p furca -- release plan` says whether this repository is ready to
release, from its own `release.toml`. A release is `furca release run` from a
clean `main`, with the installed CLI rather than one in `target/`: the gate
rebuilds `target/` while the run waits for it.
