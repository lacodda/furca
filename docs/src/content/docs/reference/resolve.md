---
title: resolve
description: Resolve a file in conflict and stage it - as the working tree has it, one side whole, or each conflict from one side.
---

```sh
furca resolve PATH [--take ours|theirs|both|base | --ours | --theirs | --delete | --force] [-C PATH] [--json]
```

Settles one file a merge, a cherry-pick, a revert, a rebase or a stash left in
conflict, and stages it: the index holds the one version chosen, and the working tree
has it, both written by your own `git` through the repository's filters and line
endings.

| Option | What the file becomes |
| --- | --- |
| *(none)* | The file as the working tree has it now - edited by hand, by another tool, by an assistant. Refused while it still holds git's markers. |
| `--force` | The same, markers and all. |
| `--take SIDE` | Every conflicting block from one side - `ours`, `theirs`, `both` (ours, then theirs) or `base`, the common ancestor's lines - and every change that does not conflict from both. |
| `--ours` | Our file whole, as `git checkout --ours` takes it: their changes to it are left out, the ones that did not conflict too. Deleted, if our side deleted it. |
| `--theirs` | Their file whole, on the same terms. |
| `--delete` | No file at all. |

```console
$ furca resolve src/client.rs
furca: src/client.rs still holds conflict markers, from line 4: resolve them, or take it as it is anyway

$ furca resolve src/client.rs --take both
Resolved src/client.rs, every conflict with both sides, ours first.
1 path left in conflict.

$ furca resolve src/legacy.rs --ours
Resolved src/legacy.rs with our file whole.
Nothing left in conflict: furca continue goes on.
```

A written file keeps the line endings our side's file had: `\r\n` stays `\r\n`. A
file one side deleted, or one that is not text, has no blocks to take: `--take` is
refused for it, in words that name the options that are left. The markers are git's
own, seven `<` closed by seven `>` further down; a line of `=======` alone is not one,
since it underlines a Markdown heading as often as it parts a conflict.

## JSON

```console
$ furca resolve src/client.rs --take both --json
{
  "path": "src/client.rs",
  "took": "blocks",
  "each": "both",
  "left": [{ "path": "src/legacy.rs", "kind": "deleted-by-them" }],
  "halfway": "merge"
}
```

`took` is `as-is`, `ours`, `theirs`, `blocks` or `delete`; `each` the side every
conflicting block was taken from, for `blocks`; `left` the paths still in conflict;
`halfway` the operation to go on with once nothing is left.

## Exit status

`0` when the file is resolved and staged; `2` when it is not in conflict, still holds
markers, or git refused.

## Related

- [conflicts](/furca/reference/conflicts/) - what is left, and one file's conflicting blocks.
- [continue](/furca/reference/continue/) - goes on once nothing is left.
