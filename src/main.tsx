import React from 'react'
import ReactDOM from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import App from '@/app/App'
import '@/i18n'
import '@/styles.css'

// The window is created hidden (tauri.conf.json) and shown from here, once
// the document has painted its splash. Shown before that, the webview would
// flash white for as long as the bundle takes to arrive. Outside Tauri - a
// test, a browser tab - there is no window to show.
if ('__TAURI_INTERNALS__' in window) {
  getCurrentWindow()
    .show()
    .catch(() => undefined)
}

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

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
