import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp } from 'lucide-react'
import { defaultKeymap, history, historyKeymap } from '@codemirror/commands'
import { syntaxHighlighting } from '@codemirror/language'
import { Change, diff as characters, MergeView } from '@codemirror/merge'
import { Compartment, EditorState, RangeSetBuilder, StateEffect, StateField, type Extension, type Range } from '@codemirror/state'
import { Decoration, EditorView, highlightSpecialChars, keymap, lineNumbers, WidgetType } from '@codemirror/view'
import { Button } from '@/components/ui/button'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import type { Conflict } from '@/lib/generated/Conflict'
import {
  conflictCount,
  lineStarts,
  markersIn,
  offsets,
  pickedText,
  shownTexts,
  startingResult,
  type Labels,
  type Pick,
  type Shown,
  type Span,
} from '@/lib/merge'
import { baseTheme, furcaHighlight, languageOf, styleNonce } from './editor'

/*
 * A conflict resolved in three panes: our file and theirs above, kept level
 * by CodeMirror's merge view with the core's blocks for its chunks, and the
 * result below, which is the file being written. Loaded on its own with the
 * first conflict opened, as a diff is.
 *
 * The result starts with every block that goes in on its own already in and
 * each conflict written out between git's markers (src/lib/merge.ts). A bar
 * over a conflict takes ours, theirs, both or the ancestor's lines in their
 * place; the text can be edited by hand too. Whatever holds no markers any
 * more is resolved, and the file can be saved once nothing does.
 */

/** Tells the result where conflict `index` is now. */
const setSpan = StateEffect.define<{ index: number; span: Span }>()

/** The conflicts' places in the result, moved with every edit. Text typed
 * at either edge of one goes into it. */
function spansField(first: Span[]) {
  return StateField.define<Span[]>({
    create: () => first,
    update(spans, tr) {
      let next = tr.docChanged
        ? spans.map((span) => ({ from: tr.changes.mapPos(span.from, -1), to: tr.changes.mapPos(span.to, 1) }))
        : spans
      for (const effect of tr.effects) {
        if (effect.is(setSpan)) next = next.map((span, at) => (at === effect.value.index ? effect.value.span : span))
      }
      return next
    },
  })
}

/** What a conflict's bar does when a side is chosen. */
type Take = (view: EditorView, index: number, pick: Pick) => void

/** The bar over a conflict in the result: which one it is, whether it is
 * resolved, and the four ways to resolve it. */
class ConflictBar extends WidgetType {
  private readonly index: number
  private readonly total: number
  private readonly resolved: boolean
  private readonly words: BarWords
  private readonly take: Take

  constructor(index: number, total: number, resolved: boolean, words: BarWords, take: Take) {
    super()
    this.index = index
    this.total = total
    this.resolved = resolved
    this.words = words
    this.take = take
  }

  override eq(other: ConflictBar): boolean {
    return other.index === this.index && other.total === this.total && other.resolved === this.resolved && other.words === this.words
  }

  override toDOM(view: EditorView): HTMLElement {
    const bar = document.createElement('div')
    bar.className = this.resolved ? 'furca-conflict-bar furca-done' : 'furca-conflict-bar'
    const held = document.createElement('div')
    const title = document.createElement('span')
    title.className = 'furca-conflict-title'
    title.textContent = this.words.title(this.index + 1, this.total, this.resolved)
    held.append(title)
    for (const pick of ['ours', 'theirs', 'both', 'base'] as const) {
      const button = document.createElement('button')
      button.type = 'button'
      button.textContent = this.words.picks[pick]
      button.title = this.words.helps[pick]
      button.addEventListener('click', () => {
        this.take(view, this.index, pick)
      })
      held.append(button)
    }
    bar.append(held)
    return bar
  }

  override ignoreEvent(): boolean {
    return true
  }
}

/** The words a bar is drawn with, made once per resolver. */
interface BarWords {
  title: (at: number, of: number, resolved: boolean) => string
  picks: Record<Pick, string>
  helps: Record<Pick, string>
}

const UNRESOLVED_LINE = Decoration.line({ class: 'furca-unresolved' })
const MARKER_LINE = Decoration.line({ class: 'furca-marker' })
const RESOLVED_LINE = Decoration.line({ class: 'furca-resolved' })
const CONFLICT_LINE = Decoration.line({ class: 'furca-conflict' })

/** Whether a line is one of git's markers. */
function isMarker(text: string): boolean {
  return /^(<{7}|\|{7}|={7}|>{7})( |$)/.test(text)
}

/** The result's own drawing: a bar over each conflict, and its lines marked
 * as still to resolve - git's markers among them - or resolved. */
function resultMarks(field: StateField<Span[]>, words: BarWords, take: Take): Extension {
  return EditorView.decorations.compute([field, 'doc'], (state) => {
    const spans = state.field(field)
    const marks: Range<Decoration>[] = []
    spans.forEach((span, index) => {
      const text = state.doc.sliceString(span.from, span.to)
      const resolved = markersIn(text) === null
      const bar = Decoration.widget({ widget: new ConflictBar(index, spans.length, resolved, words, take), block: true, side: -1 })
      // The bar goes on the line the conflict starts on, which is a line's
      // start unless an edit moved it.
      marks.push(bar.range(state.doc.lineAt(span.from).from))
      if (span.to <= span.from) return
      const last = state.doc.lineAt(Math.max(span.from, span.to - 1)).number
      for (let number = state.doc.lineAt(span.from).number; number <= last; number++) {
        const line = state.doc.line(number)
        marks.push((resolved ? RESOLVED_LINE : isMarker(line.text) ? MARKER_LINE : UNRESOLVED_LINE).range(line.from))
      }
    })
    return Decoration.set(marks, true)
  })
}

/** The lines of each conflict in one side, for its pane above. */
function sideMarks(text: string, ranges: { start: number; count: number }[]): Extension {
  const doc = EditorState.create({ doc: text }).doc
  const builder = new RangeSetBuilder<Decoration>()
  for (const range of ranges) {
    for (let line = range.start; line < range.start + range.count && line < doc.lines; line++) {
      const from = doc.line(line + 1).from
      builder.add(from, from, CONFLICT_LINE)
    }
  }
  return EditorView.decorations.of(builder.finish())
}

/* Chunks that differ between the two sides read as differences, quietly; a
 * conflict reads as the thing to look at. Colour is never the only message: a
 * conflict's bar says what it is, and git's markers are in the text. */
const resolverTheme = EditorView.theme({
  '&.cm-merge-a .cm-changedLine, &.cm-merge-b .cm-changedLine': { backgroundColor: 'var(--soft)' },
  '.cm-line.furca-conflict, .cm-line.furca-unresolved': { backgroundColor: 'var(--warn-soft)' },
  '.cm-line.furca-marker': { backgroundColor: 'var(--warn-soft)', color: 'var(--warn)', fontWeight: 'var(--font-weight-semibold)' },
  '.cm-line.furca-resolved': { boxShadow: 'inset 3px 0 0 var(--good)' },
  '.furca-conflict-bar': {
    padding: '2px 0',
    fontFamily: 'var(--font-sans)',
    fontSize: 'var(--text-2xs)',
    color: 'var(--warn)',
    background: 'var(--raise)',
    borderBlock: '1px solid var(--line)',
  },
  '.furca-conflict-bar.furca-done': { color: 'var(--good)' },
  '.furca-conflict-bar > div': { position: 'sticky', left: '0.75ch', display: 'inline-flex', alignItems: 'center', gap: '0.75ch' },
  '.furca-conflict-title': { fontWeight: 'var(--font-weight-semibold)', marginRight: '0.5ch' },
  '.furca-conflict-bar button': {
    cursor: 'pointer',
    padding: '0 0.75ch',
    color: 'var(--accent)',
    background: 'transparent',
    border: '1px solid var(--line)',
    borderRadius: 'var(--radius-sm)',
  },
  '.furca-conflict-bar button:hover': { borderColor: 'var(--line-2)', color: 'var(--text)' },
})

function common(language: Compartment, readOnly: boolean): Extension {
  return [
    styleNonce,
    EditorState.readOnly.of(readOnly),
    EditorState.lineSeparator.of('\n'),
    lineNumbers(),
    highlightSpecialChars(),
    syntaxHighlighting(furcaHighlight),
    baseTheme,
    resolverTheme,
    language.of([]),
  ]
}

/** Where the result stands: how many conflicts are left, and whether any of
 * git's markers are, anywhere in it. */
interface Standing {
  left: number
  marked: boolean
}

export interface ResolverProps {
  conflict: Conflict
  labels: Labels
  saving: boolean
  onSave: (text: string) => void
}

/** The resolver: its panes drawn afresh to start over. The one who shows it
 * draws it afresh for another conflict, keyed by its stages. */
export default function Resolver(props: ResolverProps) {
  const [fresh, setFresh] = useState(0)
  return <Panes key={fresh} {...props} onStartOver={() => setFresh((n) => n + 1)} />
}

function Panes({ conflict, labels, saving, onSave, onStartOver }: ResolverProps & { onStartOver: () => void }) {
  const { t } = useTranslation()
  const top = useRef<HTMLDivElement>(null)
  const bottom = useRef<HTMLDivElement>(null)
  const result = useRef<EditorView | null>(null)
  const sides = useRef<MergeView | null>(null)
  const field = useRef<StateField<Span[]> | null>(null)
  const total = conflictCount(conflict)
  const [at, setAt] = useState(0)
  const [standing, setStanding] = useState<Standing>({ left: total, marked: total > 0 })
  const shown = useMemo(() => shownTexts(conflict), [conflict])
  const words = useMemo<BarWords>(
    () => ({
      title: (index, of, resolved) => t(resolved ? 'merge.blockResolved' : 'merge.blockConflict', { at: index, of }),
      picks: { ours: t('merge.ours'), theirs: t('merge.theirs'), both: t('merge.both'), base: t('merge.base') },
      helps: {
        ours: t('merge.takeOurs', { name: labels.ours }),
        theirs: t('merge.takeTheirs', { name: labels.theirs }),
        both: t('merge.takeBoth'),
        base: t('merge.takeBase'),
      },
    }),
    [t, labels.ours, labels.theirs],
  )
  // What the editors are built from, as it was when they were: they are
  // built once, and drawn afresh only by a new key.
  const first = useRef({ conflict, labels, shown, words })

  useEffect(() => {
    const above = top.current
    const below = bottom.current
    const { conflict, labels, shown, words } = first.current
    if (above === null || below === null || shown === null) return
    const starting = startingResult(conflict, shown, labels)
    const spans = spansField(starting.spans)
    field.current = spans
    const description = languageOf(conflict.path)
    const languages = [new Compartment(), new Compartment(), new Compartment()] as const

    // The two sides, level: the core's blocks are the chunks, so what lines
    // up is what the merge lines up, not what a diff of the two would guess.
    const oursStarts = lineStarts(shown.ours)
    const theirsStarts = lineStarts(shown.theirs)
    const changes = conflict.blocks
      .filter((block) => block.kind === 'ours' || block.kind === 'theirs' || block.kind === 'conflict')
      .map((block) => {
        const a = offsets(oursStarts, block.ours)
        const b = offsets(theirsStarts, block.theirs)
        return new Change(a.from, a.to, b.from, b.to)
      })
    const conflicts = conflict.blocks.filter((block) => block.kind === 'conflict')
    const view = new MergeView({
      a: { doc: shown.ours, extensions: [common(languages[0], true), sideMarks(shown.ours, conflicts.map((block) => block.ours))] },
      b: { doc: shown.theirs, extensions: [common(languages[1], true), sideMarks(shown.theirs, conflicts.map((block) => block.theirs))] },
      parent: above,
      highlightChanges: false,
      gutter: false,
      collapseUnchanged: { margin: 3, minSize: 4 },
      diffConfig: {
        override: (a, b) => (a === shown.ours && b === shown.theirs ? changes : characters(a, b)),
      },
    })
    sides.current = view

    const take: Take = (editor, index, pick) => replace(editor, spans, conflict, shown, index, pick)
    const stand = (state: EditorState) => {
      const now = state.field(spans)
      const left = now.filter((span) => markersIn(state.doc.sliceString(span.from, span.to)) !== null).length
      const marked = markersIn(state.doc.toString()) !== null
      setStanding((was) => (was.left === left && was.marked === marked ? was : { left, marked }))
    }
    const editor = new EditorView({
      doc: starting.text,
      extensions: [
        common(languages[2], false),
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap]),
        spans,
        resultMarks(spans, words, take),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) stand(update.state)
        }),
      ],
      parent: below,
    })
    result.current = editor

    let current = true
    if (description) {
      const loading = description.support ? Promise.resolve(description.support) : description.load()
      loading.then(
        (support) => {
          if (!current) return
          view.a.dispatch({ effects: languages[0].reconfigure(support) })
          view.b.dispatch({ effects: languages[1].reconfigure(support) })
          editor.dispatch({ effects: languages[2].reconfigure(support) })
        },
        () => undefined,
      )
    }
    return () => {
      current = false
      result.current = null
      sides.current = null
      field.current = null
      view.destroy()
      editor.destroy()
    }
  }, [])

  /** Shows conflict `index` in all three panes. */
  const go = (index: number) => {
    const editor = result.current
    const spans = field.current
    if (editor === null || spans === null) return
    const span = editor.state.field(spans)[index]
    if (span === undefined) return
    setAt(index)
    editor.dispatch({ selection: { anchor: span.from }, effects: EditorView.scrollIntoView(span.from, { y: 'start', yMargin: 32 }) })
    const block = conflict.blocks.filter((each) => each.kind === 'conflict')[index]
    const view = sides.current
    if (block !== undefined && view !== null && shown !== null) {
      const from = offsets(lineStarts(shown.ours), block.ours).from
      view.dom.scrollTop = Math.max(0, view.a.lineBlockAt(from).top - 32)
    }
  }

  /** Takes `pick` for every conflict still to resolve: from the last, so the
   * places of those before it do not move. */
  const takeTheRest = (pick: Pick) => {
    const editor = result.current
    const spans = field.current
    if (editor === null || spans === null || shown === null) return
    for (let index = editor.state.field(spans).length - 1; index >= 0; index--) {
      const span = editor.state.field(spans)[index]
      if (span === undefined || markersIn(editor.state.doc.sliceString(span.from, span.to)) === null) continue
      replace(editor, spans, conflict, shown, index, pick)
    }
  }

  const done = !standing.marked

  return (
    <Splitter orientation="vertical" defaultSizes={[45, 55]} className="h-full min-h-0">
      <SplitterPane min={15}>
        <div className="flex h-full min-h-0 flex-col">
          <div className="grid shrink-0 grid-cols-2 border-b border-line bg-raise text-xs">
            <span className="truncate px-3 py-1.5 font-semibold" title={labels.ours}>
              {t('merge.oursPane', { name: labels.ours })}
            </span>
            <span className="truncate border-l border-line px-3 py-1.5 font-semibold" title={labels.theirs}>
              {t('merge.theirsPane', { name: labels.theirs })}
            </span>
          </div>
          <div ref={top} className="furca-diff min-h-0 flex-1 overflow-hidden" />
        </div>
      </SplitterPane>
      <SplitterHandle label={t('merge.resizeResult')} />
      <SplitterPane min={20} className="border-t border-line">
        <div className="flex h-full min-h-0 flex-col">
          <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line bg-raise px-3 py-1 text-xs">
            <span className="font-semibold">{t('merge.result')}</span>
            <span role="status" className={done ? 'text-good' : 'text-warn'}>
              {done ? t('merge.allResolved') : t('merge.left', { count: standing.left, total })}
            </span>
            <Button variant="icon" size="icon-xs" aria-label={t('merge.previous')} title={t('merge.previous')} onClick={() => go(Math.max(0, at - 1))}>
              <ChevronUp aria-hidden />
            </Button>
            <Button variant="icon" size="icon-xs" aria-label={t('merge.next')} title={t('merge.next')} onClick={() => go(Math.min(total - 1, at + 1))}>
              <ChevronDown aria-hidden />
            </Button>
            <span className="ml-auto text-dim">{t('merge.forTheRest')}</span>
            <Button size="xs" variant="ghost" disabled={done} onClick={() => takeTheRest('ours')}>
              {t('merge.ours')}
            </Button>
            <Button size="xs" variant="ghost" disabled={done} onClick={() => takeTheRest('theirs')}>
              {t('merge.theirs')}
            </Button>
            <Button size="xs" variant="ghost" onClick={onStartOver}>
              {t('merge.startOver')}
            </Button>
            <Button
              size="xs"
              variant="primary"
              disabled={!done || saving}
              title={done ? t('merge.saveHelp') : t('merge.saveBlocked')}
              onClick={() => {
                const editor = result.current
                if (editor !== null) onSave(editor.state.doc.toString())
              }}
            >
              {t('merge.save')}
            </Button>
          </div>
          <div ref={bottom} className="furca-result min-h-0 flex-1 overflow-hidden" />
        </div>
      </SplitterPane>
    </Splitter>
  )
}

/** Puts the side `pick` takes in place of conflict `index`, as one edit the
 * history can take back. */
function replace(editor: EditorView, spans: StateField<Span[]>, conflict: Conflict, shown: Shown, index: number, pick: Pick): void {
  const span = editor.state.field(spans)[index]
  if (span === undefined) return
  let text = pickedText(conflict, shown, index, pick)
  // A side's last line, with nothing after it there, may come in before more
  // lines here: it takes a newline then.
  if (text !== '' && !text.endsWith('\n') && span.to < editor.state.doc.length) text += '\n'
  editor.dispatch({
    changes: { from: span.from, to: span.to, insert: text },
    effects: setSpan.of({ index, span: { from: span.from, to: span.from + text.length } }),
    userEvent: 'input.pick',
  })
}
