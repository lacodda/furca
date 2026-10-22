# 0009 — The IPC types are generated from Rust

Date: 2026-10-22
Status: Accepted.

## Context

The window calls `furca-desktop` by name and gets JSON back. Every shape on that wire is a Rust type first - most of them `furca-core`'s own, `Refs`, `Stash`, `Standing` - and the window needs the same shape in TypeScript. kilna wrote its 145 wire types twice and kept them in step by hand; they drifted field by field, each drift was found by someone looking at the wrong screen, and kilna replaced the hand-kept mirror with generated types after 1,550 lines of it (its ADR 0042). furca's window starts with a dozen types and grows by a dozen a version: the graph's chunks, a commit's files, a diff.

## Decision

**Rust is the one source.** Every type that crosses the wire derives `ts_rs::TS` beside `Serialize`, and the TypeScript is generated into `src/lib/generated/`, one file per type.

- **The library's types derive it behind a feature.** `furca-core` gains an optional `ts` feature that adds the derive; the desktop crate turns it on, and anyone building `furca-core` from crates.io without it gets no new dependency.
- **The folder is held by a test.** `src-tauri/tests/bindings.rs` exports every type the commands take and return into a temporary directory and compares it file by file with the committed folder: a stale folder fails `cargo test`, and `FURCA_BLESS=1 cargo test -p furca-desktop --test bindings` writes it again. Nothing is written by an ordinary run.
- **Numbers are numbers.** 64-bit integers generate as `number` (`TS_RS_LARGE_INT` in `.cargo/config.toml`): JSON carries plain numbers, and every count and timestamp furca sends is far below 2^53.

## Consequences

**Positive.** A change to a type the window reads changes the window's type in the same commit, or the gate says so, and the TypeScript compiler then names every place that assumed the old shape. The CLI's `--json` and the window are one shape, because they are one type.

**Negative.** `furca-core` carries a `cfg_attr` on every public type the window reads, and the desktop build compiles `ts-rs`. A type the window needs from a crate that does not derive it is wrapped in one of furca's own.
