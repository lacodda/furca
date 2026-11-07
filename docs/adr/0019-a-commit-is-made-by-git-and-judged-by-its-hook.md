# 0019 — A commit is made by git and judged by its hook

Date: 2026-11-07
Status: Accepted.

## Context

The window can now stage and commit. Three things in that could quietly become furca's own copy of something git, or a person's own setup, already has: which rules a message has to pass, how a hunk gets into the index, and what a path means.

A rule of the line already lives in a global `commit-msg` hook, which every commit passes through whoever typed it. A composer with its own list of that rule would drift from the hook, and the person would learn of the drift when a commit was refused. A public client also has no business carrying one team's rules.

## Decision

**Git makes the change, and the rules are asked for and never copied.**

- **The hook is asked as you type.** `Git::commit_msg_says` writes the message aside and runs `git hook run --ignore-missing commit-msg`: the hook `git commit` will run, wherever `core.hooksPath` puts it, in its own words. It needs git 2.36; an older git has no `git hook`, nothing is asked, and the commit still runs the hook. It goes in no command log, because it changes nothing and runs each time typing pauses.
- **The convention has one reader.** `furca_release::message` reads a message with `git-conventional`, the parser the release engine reads commits with, so what the composer passes is what the engine understands. `furca message check` is the same function for a hook or an assistant. The composer warns; only an empty message stops the button, because git would refuse it too, and whoever wants the convention enforced has a hook for that.
- **A hunk is a patch, applied by git.** `git apply --cached` puts it in. Git refuses a patch that does not fit the index, which is what makes a stale screen safe. Writing the new blob with `hash-object` and `update-index` would work too, with nothing to refuse it. The patch is built from the bytes of both sides, not from the window's text, where whatever was not UTF-8 has become U+FFFD; only `Modified` files have hunks to offer, since a file added or deleted whole is one hunk with a button of its own.
- **A path is a path.** Paths go on standard input with `--literal-pathspecs` and `--pathspec-file-nul`: a file called `[a].txt` is that file and not `a.txt`, and a list of thousands is one run. A message goes on standard input too, so its length meets no command line. This needs git 2.25.
- **Nothing is held that git can be asked.** After each change the list is read again, rather than the window working out how the change moved it.

## Consequences

**Positive.** A rule added to a hook shows up in the composer the same day. Line endings, odd bytes and odd names come through a hunk unchanged, because git never sees them through furca's text. The window has no idea what the line's rules are, so it is as usable anywhere else.

**Negative.** Typing starts a process each time it pauses, and on Windows a shell hook behind it. That is why it runs off the thread that draws, for the latest message only. A hook with side effects would run more often than it expects; `commit-msg` is documented as taking a file and saying yes or no, and that is what it is asked.
