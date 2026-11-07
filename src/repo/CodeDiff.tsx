import { useEffect, useRef } from 'react'
import { Change, diff as characters, MergeView, unifiedMergeView, type DiffConfig } from '@codemirror/merge'
import { Compartment, EditorState, type Extension, type StateEffect } from '@codemirror/state'
import { Decoration, EditorView, highlightSpecialChars, lineNumbers, WidgetType } from '@codemirror/view'
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { tags } from '@lezer/highlight'
import type { FileDiff } from '@/lib/generated/FileDiff'
import type { Hunk } from '@/lib/generated/Hunk'
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
  '.furca-hunk-bar': {
    padding: '1px 0',
    fontFamily: 'var(--font-sans)',
    fontSize: 'var(--text-2xs)',
    color: 'var(--dim)',
    background: 'var(--raise)',
    borderBlock: '1px solid var(--line)',
  },
  // Held at the left edge of what shows: a long line scrolls the column sideways,
  // and the button must not go with it.
  '.furca-hunk-bar > div': {
    position: 'sticky',
    left: '0.75ch',
    display: 'inline-flex',
    alignItems: 'center',
    gap: '1ch',
  },
  '.furca-hunk-bar button': {
    cursor: 'pointer',
    padding: '0 0.75ch',
    color: 'var(--accent)',
    background: 'transparent',
    border: '1px solid var(--line)',
    borderRadius: 'var(--radius-sm)',
  },
  '.furca-hunk-bar button:hover': { borderColor: 'var(--line-2)', color: 'var(--text)' },
  '.furca-hunk-bar.furca-level button': { visibility: 'hidden' },
})

/** A hunk's bar: where it is and how large, with what can be done to it. It is
 * in the drawing and not around it, so it sits with the lines it is about and
 * scrolls with them. */
class HunkBar extends WidgetType {
  private readonly hunk: Hunk
  private readonly label: string
  private readonly act: ((hunk: Hunk) => void) | null

  /** With no `act`, the bar only keeps the column it is in level with the one
   * that has the button: it takes the same room, and says nothing. */
  constructor(hunk: Hunk, label: string, act: ((hunk: Hunk) => void) | null) {
    super()
    this.hunk = hunk
    this.label = label
    this.act = act
  }

  override eq(other: HunkBar): boolean {
    return (
      other.label === this.label &&
      other.act === this.act &&
      other.hunk.after.start === this.hunk.after.start &&
      other.hunk.before.start === this.hunk.before.start
    )
  }

  override toDOM(): HTMLElement {
    const bar = document.createElement('div')
    const { act, hunk } = this
    bar.className = act === null ? 'furca-hunk-bar furca-level' : 'furca-hunk-bar'
    const size = document.createElement('span')
    size.textContent = act === null ? '' : `−${String(hunk.before.count)} +${String(hunk.after.count)}`
    const button = document.createElement('button')
    button.type = 'button'
    button.textContent = this.label
    if (act !== null) {
      button.addEventListener('click', () => {
        act(hunk)
      })
    } else {
      button.tabIndex = -1
    }
    const held = document.createElement('div')
    held.append(button, size)
    bar.append(held)
    return bar
  }

  override ignoreEvent(): boolean {
    return true
  }
}

/** The bars over a text's hunks, for the editor that shows the side the hunks
 * land in. A block has to come from a value and not a view plugin, because it
 * changes how tall the document is before anything is drawn. */
function hunkBars(text: string, hunks: readonly Hunk[], side: 'before' | 'after', label: string, act: ((hunk: Hunk) => void) | null): Extension {
  const doc = EditorState.create({ doc: text, extensions: EditorState.lineSeparator.of('\n') }).doc
  const bars = hunks.map((hunk) => {
    const line = hunk[side].start
    const inside = line < doc.lines
    const widget = Decoration.widget({ widget: new HunkBar(hunk, label, act), block: true, side: inside ? -2 : 1 })
    return widget.range(inside ? doc.line(line + 1).from : doc.length)
  })
  return EditorView.decorations.of(Decoration.set(bars, true))
}

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
  /** What can be done to a hunk, when anything can: staging it, or taking it
   * out again. A commit's change has none. */
  hunkAction?: { label: string; act: (hunk: Hunk) => void }
}

export default function CodeDiff({ diff, layout, onShown, hunkAction }: CodeDiffProps) {
  const host = useRef<HTMLDivElement>(null)
  // Where the reader was: a file of the working copy is drawn again each time
  // it is saved, and that must not throw them back to its top.
  const place = useRef<{ path: string; layout: Layout; kept: StateEffect<unknown> | number } | null>(null)
  // Read when the drawing is done, not a reason to draw again.
  const shownRef = useRef(onShown)
  useEffect(() => {
    shownRef.current = onShown
  })
  // The same for the action: a new function is not a reason to draw again, only
  // whether there is one, and what it is called.
  const act = useRef(hunkAction?.act)
  useEffect(() => {
    act.current = hunkAction?.act
  })
  const hunkLabel = hunkAction?.label

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
    const barred = hunkLabel !== undefined && diff.before !== null && diff.after !== null
    const bars: Extension = barred
      ? hunkBars(after, diff.hunks, 'after', hunkLabel, (hunk) => {
          act.current?.(hunk)
        })
      : []
    const levels: Extension = barred ? hunkBars(before, diff.hunks, 'before', hunkLabel, null) : []

    // One editor scrolls itself, and keeps its place by what is at the top of
    // the screen: lines fold and unfold, so a height means little. Two kept
    // level are scrolled together by the merge view, by a height.
    const left = place.current !== null && place.current.path === diff.file.path && place.current.layout === layout ? place.current.kept : null
    const scrollTo = typeof left === 'number' || left === null ? undefined : left

    const views: EditorView[] = []
    let destroy: () => void
    let keep: () => StateEffect<unknown> | number
    if (diff.before === null || diff.after === null) {
      // A file added or deleted whole: one column of it, every line marked.
      const view = new EditorView({
        doc: diff.after === null ? before : after,
        extensions: [common(language, loaded), EditorView.editorAttributes.of({ class: diff.after === null ? 'furca-removed' : 'furca-added' })],
        parent,
        scrollTo,
      })
      views.push(view)
      destroy = () => view.destroy()
      keep = () => view.scrollSnapshot()
    } else if (layout === 'split') {
      const view = new MergeView({
        a: { doc: before, extensions: [common(language, loaded), levels] },
        b: { doc: after, extensions: [common(language, loaded), bars] },
        parent,
        highlightChanges: true,
        gutter: true,
        collapseUnchanged: collapse,
        diffConfig: config,
      })
      views.push(view.a, view.b)
      destroy = () => view.destroy()
      keep = () => view.dom.scrollTop
      if (typeof left === 'number') view.dom.scrollTop = left
    } else {
      const view = new EditorView({
        doc: after,
        extensions: [
          common(language, loaded),
          bars,
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
        scrollTo,
      })
      views.push(view)
      destroy = () => view.destroy()
      keep = () => view.scrollSnapshot()
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
      place.current = { path: diff.file.path, layout, kept: keep() }
      destroy()
    }
  }, [diff, layout, hunkLabel])

  return <div ref={host} className="furca-diff h-full min-h-0 overflow-hidden" />
}
