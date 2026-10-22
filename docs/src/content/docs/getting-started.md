---
title: Getting Started
description: Install the furca CLI and read your first repository.
---

## Install

The `furca` CLI installs from the release. The desktop window builds from a
checkout until its installers are signed - see [The window](/furca/guides/the-window/).

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

Or download the archive for your platform from
[Releases](https://github.com/lacodda/furca/releases/latest) (Windows x86_64,
Linux x86_64, macOS arm64), unpack it and put `furca` on your `PATH`.

### Installer options

| Variable | Effect |
| --- | --- |
| `FURCA_VERSION` | Install this tag (`vX.Y.Z`) instead of the latest release. |
| `FURCA_INSTALL_DIR` | Install here instead of `%LOCALAPPDATA%\Programs\furca` (Windows) or `~/.local/bin` (macOS, Linux). |
| `FURCA_NO_PATH` | Leave the `PATH` alone: the Windows installer does not add the directory, the other says nothing about it. For an install into a scratch directory. |

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

Every command takes `--json` for scripts and assistants, and those that read
one repository take `-C PATH` to read one other than the one you are in. It
exits 0 when it did what was asked, 1 when the answer is no - a release plan
with something in its way, a release run that stopped, CI that is not green,
a repository with work not pushed - and 2 when it could not answer at all. See [status](/furca/reference/status/),
[refs](/furca/reference/refs/), [log](/furca/reference/log/),
[push](/furca/reference/push/), [checks](/furca/reference/checks/),
[line](/furca/reference/line/) and [release](/furca/reference/release/).

In a GitHub Actions workflow, the repository's own action installs the CLI
from a release - see [release in CI](/furca/reference/release/#in-ci).

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
