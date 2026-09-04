# 0002 — One core, three doors

Date: 2026-09-03
Status: Accepted

## Context

Furca ships a desktop app, but releases in this product line are driven from a terminal by an assistant: version proposals, changelog generation, gates, tagging and publishing all happen as CLI invocations, not as clicks in a window. If the engine only spoke Tauri's command protocol, none of that automation would be possible without either scripting the GUI or duplicating the engine's logic in a second implementation.

## Decision

`furca-core` is a plain Rust library with no knowledge of Tauri or of being run from a terminal. It knows about repositories, refs, diffs and the release pipeline — nothing about how it is invoked.

Everything that invokes it is a thin door:

- **`furca`** — the CLI, printing JSON, built for scripts and assistants.
- **`furca-desktop`** — the Tauri app, wrapping the same calls as `#[tauri::command]` functions.
- **An MCP server** — planned, not yet built: the third door, for an assistant to call the engine directly as tools rather than by shelling out to the CLI.

A door translates between its transport (JSON on stdout, Tauri's IPC, MCP's tool protocol) and `furca-core`'s plain Rust API. It contains no logic of its own beyond that translation.

## Consequences

**Positive.** The engine is scriptable first: `furca status`, `furca release plan`, and eventually an MCP tool call are all calling the same code the desktop window calls, so there is exactly one place a behavior can be wrong. A new door — a web API, a second CLI shape — is a translation layer, not a reimplementation.

**Negative.** Anything that feels natural as "a Tauri thing" (e.g., a native file dialog) has to stay in the door, not leak into the core, even when it would be one line shorter to reach for it directly. The core cannot assume a window, a terminal, or an assistant on the other end.
