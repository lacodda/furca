import { useEffect, useRef } from 'react'
import { Change, diff as characters, MergeView, unifiedMergeView, type DiffConfig } from '@codemirror/merge'
import { Compartment, EditorState, type Extension } from '@codemirror/state'
import { EditorView, highlightSpecialChars, lineNumbers } from '@codemirror/view'
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { tags } from '@lezer/highlight'
import type { FileDiff } from '@/lib/generated/FileDiff'
import { hunkChanges, refine } from '@/lib/changes'

/*
 * A file's change in CodeMirror: two columns kept level, or one with the
 * removed lines over the added ones. This module is loaded on its own when
 * the first diff is opened - CodeMirror is the largest thing the window
 * carries, and the first frame needs none of it.
 *
 * The lines that changed are the core's (`Repository::diff`), the ones git
 * shows; CodeMirror only works out which characters inside them differ.
 * Unchanged stretches fold away, so a long file costs what its changes
 * cost, and the editor draws only the lines on the screen.
 */

export type Layout = 'split' | 'inline'

/** Unchanged lines kept around a change before the rest folds away. */
const CONTEXT = 3

const furcaHighlight = HighlightStyle.define([
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

/* Colour is never the only message: a removed line carries `−` in its gutter
 * and an added one `+`, so the change reads in a screenshot and for the
 * reader who tells the two tints apart by nothing. */
const furcaTheme = EditorView.theme({
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
  '&.cm-merge-a .cm-changedLine, .cm-deletedChunk, &.furca-removed .cm-line': { backgroundColor: 'var(--bad-soft)' },
  '&.cm-merge-b .cm-changedLine, .cm-inlineChangedLine, &.furca-added .cm-line': { backgroundColor: 'var(--good-soft)' },
  '&.cm-merge-a .cm-changedText, .cm-deletedChunk .cm-deletedText': {
    background: 'color-mix(in oklab, var(--bad) 32%, transparent)',
  },
  '&.cm-merge-b .cm-changedText': { background: 'color-mix(in oklab, var(--good) 32%, transparent)' },
  '.cm-changeGutter': { width: '1.5ch', paddingLeft: '0', textAlign: 'center' },
  '&.cm-merge-a .cm-changedLineGutter, .cm-deletedLineGutter': { background: 'var(--bad-soft)', color: 'var(--bad)' },
  '&.cm-merge-b .cm-changedLineGutter, .cm-inlineChangedLineGutter': { background: 'var(--good-soft)', color: 'var(--good)' },
  '&.cm-merge-a .cm-changedLineGutter::before, .cm-deletedLineGutter::before': { content: '"−"' },
  '&.cm-merge-b .cm-changedLineGutter::before, .cm-inlineChangedLineGutter::before': { content: '"+"' },
  '&.furca-removed .cm-lineNumbers .cm-gutterElement::before': { content: '"− "', color: 'var(--bad)' },
  '&.furca-added .cm-lineNumbers .cm-gutterElement::before': { content: '"+ "', color: 'var(--good)' },
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

/** What every editor of a diff carries: read-only text with line numbers,
 * a `\r` kept as a mark rather than a line break, and the line's colours. */
function common(language: Compartment, support: Extension): Extension {
  return [
    nonce === '' ? [] : EditorView.cspNonce.of(nonce),
    EditorState.readOnly.of(true),
    EditorState.lineSeparator.of('\n'),
    lineNumbers(),
    highlightSpecialChars(),
    syntaxHighlighting(furcaHighlight),
    furcaTheme,
    language.of(support),
  ]
}

/** The language a file is written in, by its name; `null` for one
 * CodeMirror does not know. */
function languageOf(path: string): LanguageDescription | null {
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

/** The text a side shows: line ends of `\r\n` lose the `\r`, which the
 * header says in words instead of a mark on every line. A `\r` anywhere else
 * stays, drawn as a mark. */
function shown(text: string | null): string {
  return (text ?? '').replace(/\r(?=\n|$)/g, '')
}

export interface CodeDiffProps {
  diff: FileDiff
  layout: Layout
  /** Told once the change is drawn, highlighted, on the screen. */
  onShown?: () => void
}

export default function CodeDiff({ diff, layout, onShown }: CodeDiffProps) {
  const host = useRef<HTMLDivElement>(null)
  // Read when the drawing is done, not a reason to draw again.
  const shownRef = useRef(onShown)
  useEffect(() => {
    shownRef.current = onShown
  })

  useEffect(() => {
    const parent = host.current
    if (!parent) return
    const before = shown(diff.before)
    const after = shown(diff.after)
    const language = new Compartment()
    const description = languageOf(diff.file.path)
    // Loaded already - `prepare` started it with the read - goes in from
    // the start; otherwise it follows the text.
    const loaded = description?.support ?? []
    const config: DiffConfig = {
      // The core's hunks for these two texts; anything else - there is
      // nothing else in a read-only view, but the editor may ask - its own.
      // The editor wants its own Change objects back, not their shape.
      override: (a, b) =>
        a === before && b === after
          ? refine(a, b, hunkChanges(a, b, diff.hunks), (x, y) => characters(x, y, { scanLimit: 500, timeout: 20 })).map(
              (part) => new Change(part.fromA, part.toA, part.fromB, part.toB),
            )
          : characters(a, b),
    }
    const collapse = { margin: CONTEXT, minSize: CONTEXT + 1 }

    const views: EditorView[] = []
    let destroy: () => void
    if (diff.before === null || diff.after === null) {
      // A file added or deleted whole: one column of it, every line marked.
      const view = new EditorView({
        doc: diff.after === null ? before : after,
        extensions: [common(language, loaded), EditorView.editorAttributes.of({ class: diff.after === null ? 'furca-removed' : 'furca-added' })],
        parent,
      })
      views.push(view)
      destroy = () => view.destroy()
    } else if (layout === 'split') {
      const view = new MergeView({
        a: { doc: before, extensions: common(language, loaded) },
        b: { doc: after, extensions: common(language, loaded) },
        parent,
        highlightChanges: true,
        gutter: true,
        collapseUnchanged: collapse,
        diffConfig: config,
      })
      views.push(view.a, view.b)
      destroy = () => view.destroy()
    } else {
      const view = new EditorView({
        doc: after,
        extensions: [
          common(language, loaded),
          unifiedMergeView({
            original: before,
            highlightChanges: true,
            gutter: true,
            mergeControls: false,
            syntaxHighlightDeletions: true,
            collapseUnchanged: collapse,
            diffConfig: config,
          }),
        ],
        parent,
      })
      views.push(view)
      destroy = () => view.destroy()
    }

    // A language not loaded yet arrives after the text: the change is
    // readable at once, and coloured a moment later.
    let current = true
    let frame = 0
    const told = () => {
      frame = requestAnimationFrame(() => {
        frame = requestAnimationFrame(() => {
          if (current) shownRef.current?.()
        })
      })
    }
    if (description && !description.support) {
      description.load().then(
        (support) => {
          if (!current) return
          for (const view of views) view.dispatch({ effects: language.reconfigure(support) })
          told()
        },
        () => {
          if (current) told()
        },
      )
    } else {
      told()
    }

    return () => {
      current = false
      cancelAnimationFrame(frame)
      destroy()
    }
  }, [diff, layout])

  return <div ref={host} className="furca-diff h-full min-h-0 overflow-hidden" />
}
