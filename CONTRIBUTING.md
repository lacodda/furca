# Contributing to furca

## Development

Requires Rust, Node 22+ and pnpm.

```sh
pnpm install
pnpm tauri dev            # run the desktop app
cargo test                # engine and CLI tests
```

To build only the CLI:

```sh
cargo run -p furca -- status
```
