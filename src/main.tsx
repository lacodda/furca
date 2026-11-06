import React from 'react'
import ReactDOM from 'react-dom/client'
import { CSPProvider } from '@base-ui/react/csp-provider'
import { overlayScrollbars } from 'dowel-ui'
import App from '@/app/App'
import { api } from '@/lib/api'
import '@/i18n'
import '@/styles.css'

// The window is on the screen already, in the theme's ground, before this
// webview started (ADR 0011); a measurement hears when the script runs.
if ('__TAURI_INTERNALS__' in window) void api.measured('script')

/*
 * The webview's own right-click menu does not belong in a desktop app.
 *
 * "Back", "Reload", "Save as", "Inspect" are a browser's answers, and the
 * first two can lose what is on the screen. Inside a text field the menu is
 * the only way to cut, copy and paste with the mouse, so a field keeps it, as
 * does a selection someone has just made in order to copy it.
 */
document.addEventListener('contextmenu', (event) => {
  const target = event.target
  if (!(target instanceof Element)) return
  if (target.closest('input, textarea, [contenteditable=""], [contenteditable="true"]')) return
  if ((window.getSelection()?.toString() ?? '') !== '') return
  event.preventDefault()
})

// The engine draws a bar that takes no width, where the window asks it to
// (`--enable-features=OverlayScrollbar` in tauri.conf.json); the theme stands
// aside on what this measures, and keeps its own strip where it is not so.
overlayScrollbars()

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    {/* Base UI's run-time styles are refused by the window's CSP; their
        rules are in styles.css instead. */}
    <CSPProvider disableStyleElements>
      <App />
    </CSPProvider>
  </React.StrictMode>,
)
