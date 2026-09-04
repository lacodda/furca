---
title: status
description: Print a repository's HEAD summary as JSON.
---

```sh
furca status [PATH]
```

Opens the repository containing `PATH` (or the current directory, if omitted)
and prints its `HEAD` state as pretty-printed JSON:

```json
{
  "branch": "main",
  "commit": "3f1c2b9a4e5d6f7081920a3b4c5d6e7f80910203",
  "detached": false
}
```

`branch` is `null` when `HEAD` is detached. `commit` is `null` for a
repository with no commits yet.

## Related

- [The engine](/furca/guides/the-engine/) — how this command's answer is computed.
