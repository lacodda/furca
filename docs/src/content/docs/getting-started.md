---
title: Getting Started
description: Install the furca CLI and read your first repository.
---

## Install

The `furca` CLI ships today; the desktop window follows in a later release.

One line on Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/lacodda/furca/main/tools/install.ps1 | iex
```

One line on macOS / Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/lacodda/furca/main/tools/install.sh | sh
```

:::caution[On Windows, use the PowerShell line]
`install.sh` carries the macOS and Linux builds only. Running it from Git Bash,
MSYS2 or Cygwin stops with a pointer to `install.ps1` rather than installing
anything.
:::

Via cargo:

```bash
cargo install furca
```

Or download the archive for your platform from
[Releases](https://github.com/lacodda/furca/releases/latest) (Windows x86_64,
Linux x86_64, macOS arm64), unpack it and put `furca` on your `PATH`.

### Installer options

| Variable | Effect |
| --- | --- |
| `FURCA_VERSION` | Install this tag, such as `v0.1.0`, instead of the latest release. |
| `FURCA_INSTALL_DIR` | Install here instead of `%LOCALAPPDATA%\Programs\furca` (Windows) or `~/.local/bin` (macOS, Linux). |

## First look

Inside any repository:

```console
$ furca status
On main at 171154d, tracking origin/main

$ furca log -n 2
171154d 2026-09-17 Ada Author  docs: fill the readme out as a shopfront
d402e85 2026-09-17 Ada Author  chore: point components.json at the dowel registry
... more history; raise --limit (now 2)
```

Every command takes `--json` for scripts and assistants, and `-C PATH` to read
a repository other than the one you are in. See
[status](/furca/reference/status/), [refs](/furca/reference/refs/) and
[log](/furca/reference/log/).

## Use it as a library

The CLI is a thin door onto `furca-core`, a plain Rust library with no terminal
or window of its own:

```bash
cargo add furca-core
```

```rust
let repo = furca_core::Repository::open(".")?;
let log = repo.log(furca_core::Tips::All, 500)?;
```

## Build from source

You need Rust; the desktop shell also needs Node 22+ and pnpm. See
[CONTRIBUTING.md](https://github.com/lacodda/furca/blob/main/CONTRIBUTING.md).
