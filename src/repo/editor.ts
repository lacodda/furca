import { HighlightStyle, LanguageDescription } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { EditorView } from '@codemirror/view'
import { tags } from '@lezer/highlight'

/*
 * What every CodeMirror editor of the window shares - a diff's, the
 * resolver's: the colours of code, the editor's ground and gutters, the
 * nonce its styles need, and the language a file is written in. Loaded with
 * the first of them, never with the first frame.
 */

export const furcaHighlight = HighlightStyle.define([
  { tag: [tags.keyword, tags.controlKeyword, tags.modifier, tags.operatorKeyword, tags.definitionKeyword, tags.moduleKeyword], color: 'var(--syntax-keyword)' },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.character, tags.escape], color: 'var(--syntax-string)' },
  { tag: [tags.number, tags.bool, tags.null, tags.atom, tags.unit], color: 'var(--syntax-number)' },
  { tag: [tags.comment, tags.lineComment, tags.blockComment, tags.docComment], color: 'var(--syntax-comment)', fontStyle: 'italic' },
  { tag: [tags.function(tags.variableName), tags.function(tags.propertyName), tags.definition(tags.variableName), tags.macroName, tags.labelName], color: 'var(--syntax-name)' },
  { tag: [tags.typeName, tags.className, tags.namespace, tags.tagName, tags.standard(tags.typeName)], color: 'var(--syntax-type)' },
  { tag: [tags.punctuation, tags.bracket, tags.separator, tags.operator, tags.derefOperator], color: 'var(--syntax-punctuation)' },
  { tag: [tags.meta, tags.annotation, tags.processingInstruction, tags.attributeName, tags.self], color: 'var(--syntax-meta)' },
  { tag: tags.heading, color: 'var(--syntax-keyword)', fontWeight: 'var(--font-weight-semibold)' },
  { tag: tags.link, textDecoration: 'underline' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: 'var(--font-weight-semibold)' },
])

/** The editor's ground, type and gutters, in the window's colours. */
export const baseTheme = EditorView.theme({
  '&': {
    height: '100%',
    backgroundColor: 'var(--bg)',
    color: 'var(--text)',
    fontSize: 'var(--text-xs)',
  },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.5' },
  '.cm-content': { caretColor: 'var(--text)' },
  '&.cm-focused': { outline: 'none' },
  '.cm-gutters': { backgroundColor: 'var(--raise)', color: 'var(--faint)', borderRight: '1px solid var(--line)' },
  '.cm-lineNumbers .cm-gutterElement': { minWidth: '3ch', padding: '0 0.5ch 0 1ch' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--accent-soft)' },
  // The stretch a merge view folds away: quiet, and in the window's type.
  '.cm-collapsedLines': {
    color: 'var(--dim)',
    background: 'var(--soft)',
    fontFamily: 'var(--font-sans)',
    fontSize: 'var(--text-2xs)',
  },
  '.cm-collapsedLines:hover': { color: 'var(--text)' },
})

/**
 * The nonce Tauri gave the page's own `<style>` in index.html. CodeMirror
 * styles itself with a `<style>` of its own at run time, and the window's
 * CSP refuses one without the nonce - silently: the element is there, its
 * sheet is null, and every editor draws as bare text. Empty outside Tauri,
 * where there is no CSP to satisfy.
 */
const nonce = document.querySelector<HTMLStyleElement>('style[nonce]')?.nonce ?? ''

/** The nonce as an editor takes it; nothing outside Tauri. */
export const styleNonce = nonce === '' ? [] : EditorView.cspNonce.of(nonce)

/** The language a file is written in, by its name; `null` for one
 * CodeMirror does not know. */
export function languageOf(path: string): LanguageDescription | null {
  return LanguageDescription.matchFilename(languages, path)
}

/** Starts loading the language of the file at `path`, so it is there when
 * the file's change arrives: the two are read side by side, and an editor
 * made with its language parses the text once rather than twice. */
export function prepare(path: string): void {
  void languageOf(path)
    ?.load()
    .catch(() => undefined)
}

