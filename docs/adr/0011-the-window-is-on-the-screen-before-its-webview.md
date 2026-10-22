# 0011 — The window is on the screen before its webview

Date: 2026-10-22
Status: Accepted.

## Context

The window's budget is the one the research set out on day one: the window on the screen in under 300 ms, before any data - the data streams in after. The first measurement of the built app, on the author's machine, timed every stage from the start of the process:

| Stage | ms |
| --- | --- |
| the app's own code runs | 5 |
| the webview is ready | 350 |
| the page's script runs | 430 |
| the window state is read | 455 |
| the tab in front is read | 510 |
| the first frame with the repository | 575 |

The window was made hidden and shown by the page, so nothing was on the screen for 440 ms. Most of that is the system's webview starting - WebView2 here - which an application cannot hurry: scheda, built to paint before anything else runs, paints its first frame at about 540 ms on the same machine. Hiding the window until the page had painted was the line's habit (kilna), against a white flash in the webview.

## Decision

**The native window is made first, in the ground of the system's theme, shown, and the webview is put into it after.** The two are made apart through Tauri's `unstable` window API (`Window::add_child`), because a webview window is made whole and shown only once its webview is up.

- **The ground is the theme's.** The window reads the system's theme as soon as it exists and takes dowel's ground for it - the same two colours `index.html` writes for its splash - and the webview is given that ground as its own background, so nothing flashes white or in the other theme when the page arrives.
- **The front tab is read while the webview starts.** The read of its left column begins on a thread of its own before the webview is made, and the page's first ask for it takes that read.
- **Two numbers, one promise.** `window-on-screen` - start to the window shown - is the budget, under 300 ms. `window-first-repository` - start to the first frame that shows the repository - is measured and published with every release but not promised, as the graph of a repository without a commit-graph is: most of it is the webview's start.

## Consequences

**Positive.** A click on the icon is answered at once: the window is on the screen in about 45 ms, in its own colours, and the splash and then the repository follow inside it. The read of the first repository no longer waits for the webview; the frame with the repository moved from 575 to about 600 ms - the webview starts some 50 ms slower when its window is already visible - while the window appears 400 ms sooner.

**Negative.** The window depends on an API Tauri marks unstable, and on a hand-made pairing of window and webview that `WebviewWindow` would otherwise make: the webview follows the window's size through `auto_resize`, and a change in Tauri's multi-webview support can break it. For about 350 ms the window shows its ground and nothing else - the frame without its title bar, which the page draws.
