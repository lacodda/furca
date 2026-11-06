# 0015 — The window runs a release, and the engine decides

Date: 2026-11-06
Status: Accepted.

## Context

A release is run from a terminal, which gives it three things without asking: a place for what the gate prints, a console for every program it starts, and a person watching. The window has none of them. A console program started from it gets a black box of its own on Windows, what it prints goes nowhere, and a run takes twenty minutes during which the tab, the panel and the window's focus all move.

A panel could also be where release rules start to live twice: which button is allowed, what a red check means, what a stop means.

## Decision

**The panel draws what the engine says, and the engine says it as it goes.**

- **A run tells.** `Runner` has one `tell` in place of a `say` and a flag about standard error: a step began, a step said something, a step ended and how. The `Report` a run returns is the whole of it; this is the same run while it is happening. A terminal prints the lines and ignores the rest.
- **A machine prints to whoever shows it.** `Local` takes an `Output`: the terminal, standard error for `--json`, or a line at a time to a function. Lines are given as a terminal would have shown them, with colours gone and a carriage return starting the line over. Every program the engine starts from a window asks for no console window.
- **A stopped run is read from its checkpoint.** `Release::under_way` says where a run stopped and whether `resume` can carry it on, so a panel opened later, or a run begun in a terminal, shows the same place. A step counts as passed only with every step before it: a commit pushed after a red CI has its `green` behind it.
- **What a run would do is the plan's to say.** `Plan::would` is the list the CLI used to work out for itself, so the panel and `furca release plan` read one list.
- **The buttons follow from that, and refuse nothing the engine would allow.** Run needs a plan with no red check and a proposed number; a run past its commit can only be resumed; one before its commit starts over. The panel has no override, because the CLI has none.
- **The run lives in the app, the log in the page.** A thread of the app's holds the run and sends what it hears on the `release` event, gathered for 60 ms because a compiler prints faster than a window should be told. The page keeps it for as long as the window is open, the newest 20 000 lines of each step, and draws a screenful.
- **Both Run and Resume ask first.** Past the gate they push, tag and publish, and a tag that went out stays: one click from a list of rows is too little for that.

## Consequences

**Positive.** The window and the CLI cannot disagree about a release, because neither of them decides anything. A window closed mid-run stops the run where it is and the checkpoint knows where that was, as with a terminal closed mid-run (ADR 0007).

**Negative.** `Runner`, `Local` and `Machine` changed shape, which breaks a library user of `furca-release` before 1.0. A run has no cancel button, because the CLI has none beyond closing it. Nothing stops a window and a terminal running one repository's release at the same moment: one window runs one at a time, but the checkpoint is not a lock. A release of furca itself from the window has to run from a copy of the app outside `target/`, since the gate rebuilds it, as the CLI already does.
