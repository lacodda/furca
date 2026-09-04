---
title: Getting Started
description: Build furca from source and run the CLI or the desktop shell.
---

## Install

Coming with the first release. Builds for Windows, macOS and Linux, plus the
`furca` CLI binary, will be published on the
[Releases page](https://github.com/lacodda/furca/releases).

## Build from source

You need Rust, Node 22+ and pnpm.

```sh
git clone https://github.com/lacodda/furca.git
cd furca
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` compiles the Rust backend and opens the app window. The first
build takes a while; later ones are incremental.

To build only the CLI:

```sh
cargo run -p furca -- status
```
