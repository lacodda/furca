---
title: message
description: Read a commit message as the release engine will, for a commit-msg hook or an assistant.
---

```sh
furca message check [FILE] [--json]
```

Reads a commit message from `FILE`, or from standard input when `FILE` is left
out or `-`, with the parser the [release engine](/furca/reference/release/) reads
every commit by: what passes here is what the engine will understand later.
Comment lines are not part of the message, nor is what follows the scissors line
of `git commit --verbose`.

```console
$ furca message check .git/COMMIT_EDITMSG
not a Conventional Commit: expected `type(scope): description`
the subject is 88 characters, 72 at most
```

| Says | | Against the convention |
| --- | --- | --- |
| `empty` | Nothing to commit with. | yes |
| `not-conventional` | Not `type(scope): description`. What git writes itself - `Merge`, `Revert "`, `fixup!`, `squash!`, `amend!` - is left alone. | yes |
| `unknown-type` | In that shape, under a type other than `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`. | yes |
| `no-blank-line` | The second line is not blank, so git takes it as more of the subject. | yes |
| `long-subject` | A subject over 72 characters. | |
| `final-period` | A subject ending in a full stop. | |
| `not-english` | Letters outside the Latin alphabets. An accent in a name is fine. | |

The exit status is 1 for anything against the convention and 0 otherwise, so a
`commit-msg` hook takes it as it is and style stays a word of advice:

```sh
#!/bin/sh
furca message check "$1"
```

The [window's composer](/furca/guides/the-window/#committing) asks the same
function, and then asks your hook: a rule you keep in a `commit-msg` hook is heard
as you type, in the hook's own words, without a second copy of it anywhere.

## JSON

```console
$ furca message check --json .git/COMMIT_EDITMSG
[
  { "kind": "not-conventional" },
  { "kind": "long-subject", "length": 88, "most": 72 }
]
```
