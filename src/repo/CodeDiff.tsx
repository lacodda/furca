import { useEffect, useRef } from 'react'
import { Change, diff as characters, getChunks, getOriginalDoc, MergeView, unifiedMergeView, type DiffConfig } from '@codemirror/merge'
import { Compartment, EditorState, RangeSet, RangeSetBuilder, StateEffect, StateField, type Extension, type Text } from '@codemirror/state'
import {
  BlockType,
  Decoration,
  EditorView,
  GutterMarker,
  gutterLineClass,
  highlightSpecialChars,
  lineNumbers,
  WidgetType,
  type DecorationSet,
} from '@codemirror/view'
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import { tags } from '@lezer/highlight'
import type { FileDiff } from '@/lib/generated/FileDiff'
import type { Hunk } from '@/lib/generated/Hunk'
import { hunkChanges, refine } from '@/lib/changes'
import { isChanged, NO_PICKS, setLines, type Picks, type Side } from '@/lib/picks'

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
  '.furca-hunk-bar button.furca-bad': { color: 'var(--bad)' },
  '.furca-hunk-bar button.furca-bad:hover': { borderColor: 'var(--bad)', color: 'var(--bad)' },
  '.furca-hunk-bar.furca-level button': { visibility: 'hidden' },
  // A changed line is picked by its number: the number says it can be, and
  // a picked line carries the accent from its number across its text.
  '.cm-gutterElement.furca-pickable': { cursor: 'pointer' },
  '.cm-gutterElement.furca-pickable:hover': { color: 'var(--text)', backgroundColor: 'var(--soft)' },
  '.cm-gutterElement.furca-picked, .cm-gutterElement.furca-picked:hover': {
    color: 'var(--on-accent)',
    backgroundColor: 'var(--accent)',
  },
  '.cm-line.furca-picked, .cm-deletedLine.furca-picked': {
    boxShadow: 'inset 3px 0 0 var(--accent)',
    backgroundImage: 'linear-gradient(color-mix(in oklab, var(--accent) 18%, transparent), color-mix(in oklab, var(--accent) 18%, transparent))',
  },
  '.cm-deletedLine.furca-pickable': { cursor: 'pointer' },
})

/** A hunk's button: what it says, what it does, and whether what it does
 * cannot be taken back. */
export interface HunkAct {
  label: string
  act: (hunk: Hunk) => void
  /** Drawn in the colour of loss: discarding. */
  bad?: boolean
}

/** A hunk's bar: where it is and how large, with what can be done to it. It is
 * in the drawing and not around it, so it sits with the lines it is about and
 * scrolls with them. */
class HunkBar extends WidgetType {
  private readonly hunk: Hunk
  private readonly acts: readonly HunkAct[]
  private readonly level: boolean

  /** A `level` bar only keeps the column it is in level with the one that has
   * the buttons: it takes the same room, and says nothing. */
  constructor(hunk: Hunk, acts: readonly HunkAct[], level: boolean) {
    super()
    this.hunk = hunk
    this.acts = acts
    this.level = level
  }

  override eq(other: HunkBar): boolean {
    return (
      other.level === this.level &&
      other.acts === this.acts &&
      other.hunk.after.start === this.hunk.after.start &&
      other.hunk.before.start === this.hunk.before.start
    )
  }

  override toDOM(): HTMLElement {
    const bar = document.createElement('div')
    const { acts, hunk, level } = this
    bar.className = level ? 'furca-hunk-bar furca-level' : 'furca-hunk-bar'
    const held = document.createElement('div')
    for (const each of acts) {
      const button = document.createElement('button')
      button.type = 'button'
      button.textContent = each.label
      if (each.bad === true) button.className = 'furca-bad'
      if (level) {
        button.tabIndex = -1
      } else {
        button.addEventListener('click', () => {
          each.act(hunk)
        })
      }
      held.append(button)
    }
    const size = document.createElement('span')
    size.textContent = level ? '' : `−${String(hunk.before.count)} +${String(hunk.after.count)}`
    held.append(size)
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
function hunkBars(text: string, hunks: readonly Hunk[], side: Side, acts: readonly HunkAct[], level: boolean): Extension {
  const doc = EditorState.create({ doc: text, extensions: EditorState.lineSeparator.of('\n') }).doc
  const bars = hunks.map((hunk) => {
    const line = hunk[side].start
    const inside = line < doc.lines
    const widget = Decoration.widget({ widget: new HunkBar(hunk, acts, level), block: true, side: inside ? -2 : 1 })
    return widget.range(inside ? doc.line(line + 1).from : doc.length)
  })
  return EditorView.decorations.of(Decoration.set(bars, true))
}

/** Tells an editor which lines are picked now. */
const setPicks = StateEffect.define<Picks>()

class LineMark extends GutterMarker {
  override readonly elementClass: string
  constructor(elementClass: string) {
    super()
    this.elementClass = elementClass
  }
}
const PICKED_LINE = Decoration.line({ class: 'furca-picked' })
const PICKED_NUMBER = new LineMark('furca-picked')
const PICKABLE_NUMBER = new LineMark('furca-pickable')

interface Marked {
  picks: Picks
  lines: DecorationSet
  numbers: RangeSet<GutterMarker>
}

/** The picked lines of `side` as an editor of `doc` marks them. */
function marked(doc: Text, side: Side, picks: Picks): Marked {
  const lines = new RangeSetBuilder<Decoration>()
  const numbers = new RangeSetBuilder<GutterMarker>()
  for (const line of [...picks[side]].sort((a, b) => a - b)) {
    if (line >= doc.lines) continue
    const from = doc.line(line + 1).from
    lines.add(from, from, PICKED_LINE)
    numbers.add(from, from, PICKED_NUMBER)
  }
  return { picks, lines: lines.finish(), numbers: numbers.finish() }
}

/** What lets an editor showing `side` mark picked lines: the picks it holds,
 * the lines and numbers it marks, and the numbers that can be picked. */
function picking(side: Side, hunks: readonly Hunk[], first: Picks): { field: StateField<Marked>; extension: Extension } {
  const field = StateField.define<Marked>({
    create: (state) => marked(state.doc, side, first),
    update(value, tr) {
      for (const effect of tr.effects) if (effect.is(setPicks)) return marked(tr.state.doc, side, effect.value)
      return value
    },
    provide: (f) => [EditorView.decorations.from(f, (value) => value.lines), gutterLineClass.from(f, (value) => value.numbers)],
  })
  const pickable = gutterLineClass.compute([], (state) => {
    const numbers = new RangeSetBuilder<GutterMarker>()
    for (const hunk of hunks) {
      const range = hunk[side]
      for (let line = range.start; line < range.start + range.count && line < state.doc.lines; line++) {
        const from = state.doc.line(line + 1).from
        numbers.add(from, from, PICKABLE_NUMBER)
      }
    }
    return numbers.finish()
  })
  return { field, extension: [field, pickable] }
}

/** The line of the text before that the first line of a unified view's block
 * of removed lines is. */
function firstRemoved(view: EditorView, chunk: Element): number | null {
  const chunks = getChunks(view.state)?.chunks
  if (chunks === undefined || chunks.length === 0) return null
  const at = view.posAtDOM(chunk)
  let found = chunks[0]!
  for (const each of chunks) if (Math.abs(each.fromB - at) < Math.abs(found.fromB - at)) found = each
  return getOriginalDoc(view.state).lineAt(found.fromA).number - 1
}

/** Marks the removed lines a unified view draws in blocks of its own, which
 * no decoration reaches: picked, and those that can be. */
function markRemoved(view: EditorView, picks: Picks, hunks: readonly Hunk[]): void {
  for (const chunk of view.contentDOM.querySelectorAll('.cm-deletedChunk')) {
    const first = firstRemoved(view, chunk)
    if (first === null) continue
    chunk.querySelectorAll('.cm-deletedLine').forEach((line, at) => {
      line.classList.toggle('furca-picked', picks.before.has(first + at))
      line.classList.toggle('furca-pickable', isChanged(hunks, 'before', first + at))
    })
  }
}

/** The changed line at the height `y` on the screen in an editor showing
 * `side`: a line of its own text, or one of the removed lines a unified view
 * draws between them. `null` over anything else. */
function lineAtHeight(view: EditorView, side: Side, y: number, hunks: readonly Hunk[]): { side: Side; line: number } | null {
  for (const chunk of view.contentDOM.querySelectorAll('.cm-deletedChunk')) {
    const box = chunk.getBoundingClientRect()
    if (y < box.top || y >= box.bottom) continue
    const first = firstRemoved(view, chunk)
    let at = -1
    chunk.querySelectorAll('.cm-deletedLine').forEach((line, index) => {
      const each = line.getBoundingClientRect()
      if (y >= each.top && y < each.bottom) at = index
    })
    if (first === null || at === -1 || !isChanged(hunks, 'before', first + at)) return null
    return { side: 'before', line: first + at }
  }
  const block = view.elementAtHeight(y - view.documentTop)
  if (block.type !== BlockType.Text) return null
  const line = view.state.doc.lineAt(block.from).number - 1
  return isChanged(hunks, side, line) ? { side, line } : null
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
  /** What can be done to a hunk, when anything can: staging it, taking it out
   * again, discarding it. A commit's change has none. */
  hunkActs?: readonly HunkAct[]
  /** The lines picked. */
  picks?: Picks
  /** Picks lines and lets them go, by their numbers; with none, no line can be
   * picked. */
  onPicks?: (picks: Picks) => void
}

export default function CodeDiff({ diff, layout, onShown, hunkActs, picks = NO_PICKS, onPicks }: CodeDiffProps) {
  const host = useRef<HTMLDivElement>(null)
  // Where the reader was: a file of the working copy is drawn again each time
  // it is saved, and that must not throw them back to its top.
  const place = useRef<{ path: string; layout: Layout; kept: StateEffect<unknown> | number } | null>(null)
  // Read when they are needed, not reasons to draw again: a new function is
  // not, only whether there is one and what it is called.
  const shownRef = useRef(onShown)
  const actsRef = useRef(hunkActs)
  const picksRef = useRef(picks)
  const onPicksRef = useRef(onPicks)
  useEffect(() => {
    shownRef.current = onShown
    actsRef.current = hunkActs
    onPicksRef.current = onPicks
  })
  const actLabels = hunkActs?.map((each) => `${each.label}${each.bad === true ? '!' : ''}`).join('\n')
  const pickable = onPicks !== undefined
  // The editors drawn now, with the side each shows and its picks.
  const drawn = useRef<{ view: EditorView; side: Side; field: StateField<Marked> }[]>([])

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
    const both = diff.before !== null && diff.after !== null
    // The buttons call whatever the acts are when pressed.
    const acts: HunkAct[] = (actLabels === undefined ? [] : (actsRef.current ?? [])).map((each, at) => ({
      label: each.label,
      bad: each.bad,
      act: (hunk) => actsRef.current?.[at]?.act(hunk),
    }))
    const barred = acts.length > 0 && both
    const bars: Extension = barred ? hunkBars(after, diff.hunks, 'after', acts, false) : []
    const levels: Extension = barred ? hunkBars(before, diff.hunks, 'before', acts, true) : []
    const marksBefore = picking('before', diff.hunks, picksRef.current)
    const marksAfter = picking('after', diff.hunks, picksRef.current)
    const withPicks = (marks: { extension: Extension }): Extension => (pickable && both ? marks.extension : [])

    // One editor scrolls itself, and keeps its place by what is at the top of
    // the screen: lines fold and unfold, so a height means little. Two kept
    // level are scrolled together by the merge view, by a height.
    const left = place.current !== null && place.current.path === diff.file.path && place.current.layout === layout ? place.current.kept : null
    const scrollTo = typeof left === 'number' || left === null ? undefined : left

    const views: EditorView[] = []
    let destroy: () => void
    let keep: () => StateEffect<unknown> | number
    let editors: { view: EditorView; side: Side; field: StateField<Marked> }[] = []
    if (!both) {
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
        a: { doc: before, extensions: [common(language, loaded), levels, withPicks(marksBefore)] },
        b: { doc: after, extensions: [common(language, loaded), bars, withPicks(marksAfter)] },
        parent,
        highlightChanges: true,
        gutter: true,
        collapseUnchanged: collapse,
        diffConfig: config,
      })
      views.push(view.a, view.b)
      editors = [
        { view: view.a, side: 'before', field: marksBefore.field },
        { view: view.b, side: 'after', field: marksAfter.field },
      ]
      destroy = () => view.destroy()
      keep = () => view.dom.scrollTop
      if (typeof left === 'number') view.dom.scrollTop = left
    } else {
      // The removed lines are blocks the merge view draws itself, out of reach
      // of a decoration: they are marked after each update instead.
      const removed: Extension = pickable
        ? EditorView.updateListener.of((update) => {
            markRemoved(update.view, update.state.field(marksAfter.field).picks, diff.hunks)
          })
        : []
      const view = new EditorView({
        doc: after,
        extensions: [
          common(language, loaded),
          bars,
          withPicks(marksAfter),
          removed,
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
      editors = [{ view, side: 'after', field: marksAfter.field }]
      if (pickable) markRemoved(view, picksRef.current, diff.hunks)
      destroy = () => view.destroy()
      keep = () => view.scrollSnapshot()
    }
    drawn.current = pickable ? editors : []

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
      drawn.current = []
      place.current = { path: diff.file.path, layout, kept: keep() }
      destroy()
    }
  }, [diff, layout, actLabels, pickable])

  // Picks that changed reach the editors drawn, without drawing them again.
  useEffect(() => {
    picksRef.current = picks
    for (const { view, field } of drawn.current) {
      if (view.state.field(field, false)?.picks !== picks) view.dispatch({ effects: setPicks.of(picks) })
    }
  }, [picks])

  // A line is picked by pressing its number - or, in one column, a removed
  // line itself, which has no number of its own - and a drag over more numbers
  // picks those too; Shift picks the lines from the last one pressed. A
  // pressed line that was picked is let go, and so are the lines dragged over.
  useEffect(() => {
    const parent = host.current
    if (!parent || !pickable) return
    let anchor: { side: Side; line: number } | null = null
    let stopDrag = () => {
      /* nothing is being dragged yet */
    }

    const down = (event: MouseEvent) => {
      if (event.button !== 0 || !(event.target instanceof Element)) return
      const target = event.target
      const editor = drawn.current.find(({ view }) => view.dom.contains(target))
      if (editor === undefined) return
      const onNumber = target.closest('.cm-gutters') !== null
      const onRemoved = target.closest('.cm-deletedLine') !== null
      if (!onNumber && !onRemoved) return
      const hit = lineAtHeight(editor.view, editor.side, event.clientY, diff.hunks)
      if (hit === null) return
      event.preventDefault()
      const base = picksRef.current
      const extend = event.shiftKey && anchor !== null && anchor.side === hit.side
      const from = extend && anchor !== null ? anchor.line : hit.line
      const on = extend ? true : !base[hit.side].has(hit.line)
      if (!extend) anchor = hit
      onPicksRef.current?.(setLines(base, diff.hunks, hit.side, from, hit.line, on))

      const move = (moved: MouseEvent) => {
        const over = lineAtHeight(editor.view, editor.side, moved.clientY, diff.hunks)
        if (over === null || over.side !== hit.side) return
        onPicksRef.current?.(setLines(base, diff.hunks, hit.side, from, over.line, on))
      }
      const up = () => stopDrag()
      stopDrag = () => {
        window.removeEventListener('mousemove', move)
        window.removeEventListener('mouseup', up)
      }
      window.addEventListener('mousemove', move)
      window.addEventListener('mouseup', up)
    }
    // Before the editor hears it: a press on a removed line would otherwise
    // start a selection of text.
    parent.addEventListener('mousedown', down, true)
    return () => {
      parent.removeEventListener('mousedown', down, true)
      stopDrag()
    }
  }, [diff, layout, pickable])

  return <div ref={host} className="furca-diff h-full min-h-0 overflow-hidden" />
}

