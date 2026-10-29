---
title: diff
description: Show the lines a commit changed, as patches.
---

```sh
furca diff [REVISION] [-C PATH] [-U LINES] [--json] [-- PATH...]
```

Prints the patch of every file a commit changed - `HEAD` unless a revision
names another - or of the files named after `--`, by their path after the
commit. The lines are the ones `git diff` prints for the same commit, hunk for
hunk.

```console
$ furca diff -- src/lib.rs
diff src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,5 @@
-pub fn greet() -> String {
-    "Hello".to_owned()
+pub fn greet(name: &str) -> String {
+    format!("Hello, {name}")
 }

 pub fn farewell() -> String {
```

A commit is compared with its first parent, as [`show`](../show/) explains. A
file that is not text has no lines; its patch says what it is instead:

```console
$ furca diff -- assets/logo.png
diff assets/logo.png
assets/logo.png: binary, 24.0 KB
```

A path the commit did not change is an error, exit code 2: a mistyped path is
not taken for a file without changes.

## How lines are compared

As git compares them: by the algorithm `diff.algorithm` names (Myers unless the
repository says otherwise), with git's slider heuristics deciding where a
change that could sit in two places goes. A line that only changed its ending
- `\r\n` to `\n` - is a changed line, as git shows it: a file that drifted to
another ending shows every line it moved.

## JSON

An array with one patch per file:

```console
$ furca diff --json -U 1 -- src/lib.rs
[
  {
    "file": {
      "path": "src/lib.rs",
      "status": "modified",
      "from": null,
      "similarity": null,
      "before": { "id": "a99ce086…", "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 110 },
      "after": { "id": "6e96d30e…", "mode": "file", "content": { "kind": "text", "endings": "lf" }, "size": 126 },
      "lines": { "added": 2, "removed": 2 }
    },
    "hunks": [
      {
        "before": { "start": 0, "count": 3 },
        "after": { "start": 0, "count": 3 },
        "lines": [
          { "kind": "removed", "text": "pub fn greet() -> String {" },
          { "kind": "removed", "text": "    \"Hello\".to_owned()" },
          { "kind": "added", "text": "pub fn greet(name: &str) -> String {" },
          { "kind": "added", "text": "    format!(\"Hello, {name}\")" },
          { "kind": "context", "text": "}" }
        ]
      }
    ]
  }
]
```

| Field | Meaning |
| --- | --- |
| `file` | The file as [`show --json`](../show/#json) describes it. |
| `hunks[].before`, `hunks[].after` | The lines the hunk spans on each side, context included: the first counted from 0, and how many. |
| `hunks[].lines[].kind` | `context`, `removed` or `added`. |
| `hunks[].lines[].text` | The line without its newline; a `\r` before it is kept. |

## Options

| Option | Meaning |
| --- | --- |
| `REVISION` | The commit whose lines to show. Defaults to `HEAD`. |
| `-- PATH...` | Only these files, by their path after the commit. |
| `-U, --context <LINES>` | Unchanged lines around each change. Defaults to `3`, as in git; hunks closer than twice this are one. |
| `-C, --repo <PATH>` | A path inside the repository. Defaults to `.`. |
| `--json` | Print JSON instead of text. |
