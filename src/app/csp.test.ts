import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { describe, expect, it } from 'vitest'

// The window's CSP refuses a `<style>` made at run time, and nothing says so
// - the element is in the DOM, its sheet is null. So the rules two components
// would have inserted live in the bundle's stylesheet, and this holds them
// there: a jsdom test cannot see the refusal itself.
const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8')
// Comments out, so a rule only described in prose does not count.
const css = read('../styles.css').replace(/\/\*[\s\S]*?\*\//g, '')

describe('styles the CSP would refuse at run time', () => {
  it("hides Base UI's native scrollbars from the bundle", () => {
    expect(css).toMatch(/\.base-ui-disable-scrollbar\s*\{\s*scrollbar-width:\s*none;?\s*\}/)
    expect(css).toMatch(/\.base-ui-disable-scrollbar::-webkit-scrollbar\s*\{\s*display:\s*none;?\s*\}/)
  })

  it("declares the splash's sweep from the bundle", () => {
    expect(css).toMatch(/@keyframes\s+dowel-splash-sweep\s*\{/)
  })

  it('tells Base UI not to insert its own', () => {
    expect(read('../main.tsx')).toMatch(/<CSPProvider disableStyleElements>/)
  })

  // CodeMirror's rules are made at run time, under class names it makes up,
  // so they cannot move to the bundle. Its `<style>` carries the nonce Tauri
  // gives the page's own inline style instead - which must therefore be
  // there to take it from.
  it("hands CodeMirror the nonce of the page's own style", () => {
    const diff = read('../repo/CodeDiff.tsx')
    expect(diff).toMatch(/EditorView\.cspNonce\.of\(nonce\)/)
    expect(diff).toMatch(/querySelector<HTMLStyleElement>\('style\[nonce\]'\)/)
    expect(read('../../index.html')).toMatch(/<style>/)
  })
})
