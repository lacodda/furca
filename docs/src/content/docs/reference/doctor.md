---
title: doctor
description: Say what is staged that would cost more to mend after the commit - line endings, large files, an assistant's files.
---

```sh
furca doctor [-C PATH] [--json]
```

Reads what is staged and says what is worth a word before it becomes history.
Nothing is refused and nothing is changed: each of these can be what you meant.
The composer in the [window](/furca/guides/the-window/#committing) shows the same
words as you stage.

```console
$ furca doctor
endings    src/lib.rs: LF -> CRLF
endings    notes.txt: a new file in CRLF
large      assets/demo.mov: 48.2 MB
assistant  CLAUDE.md: an assistant's file
```

| Word | What it means |
| --- | --- |
| `endings` | The file's lines end another way than they did, which shows up as every line changed; or a new file starts out in `CRLF`, which is where that begins. A `.gitattributes` stops the next one and cannot take this one out of the history. A file of one line has no ending to have changed. |
| `large` | The file is 5 MB or more and not in git LFS. Every clone carries it for good. A file in LFS is left alone, whatever its size. |
| `assistant` | A file an assistant keeps its notes or settings in: `CLAUDE.md`, `AGENTS.md`, `GEMINI.md`, `.cursorrules`, `.clinerules`, `.windsurfrules`, `.github/copilot-instructions.md`, or anything under `.claude/`, `.cursor/`, `.windsurf/`, `.gemini/`, `.codex/`, `.aider/` or `openspec/`, at any depth. Taking one out of the repository is not said. |

The exit status is 0 with nothing to say and 1 with something, so a script can
stop on it.

## JSON

```console
$ furca doctor --json
[
  { "kind": "endings", "path": "src/lib.rs", "before": "lf", "after": "crlf" },
  { "kind": "large", "path": "assets/demo.mov", "size": 50541363 },
  { "kind": "assistant", "path": "CLAUDE.md" }
]
```

`before` is `null` for a new file; `size` is in bytes; line endings are `lf`,
`crlf` or `mixed`.
