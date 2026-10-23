import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Archive, Tag as TagIcon } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { windowFor } from '@/components/ui/virtual-list'
import type { GraphRow } from '@/lib/generated/GraphRow'
import { rowAt, wantRows, type GraphView as Held } from '@/lib/graph'
import { markedPieces, type Label } from '@/lib/graph-labels'
import { short } from '@/lib/names'
import { columnWidth, draw, GEOMETRY } from './draw'
import { usePalette } from './palette'

/** The height of the row of column names. */
const HEADER = 28
/** Rows drawn beyond each edge of the view, so a fast scroll does not show
 * blank rows before the next frame. */
const OVERSCAN = 8

export interface GraphViewProps {
  path: string
  graph: Held
  /** What points at each commit. */
  labels: ReadonlyMap<string, Label[]>
  /** The commit `HEAD` is on, ringed in the graph. */
  head: string | null
  selected: number | null
  onSelect: (row: number) => void
  /** What the search looks for, marked where a row shows it. */
  query: string
  /** Rows a search found, and of those, the ones found in the message only. */
  found: ReadonlySet<number>
  inMessage: ReadonlySet<number>
}

/**
 * The history as Fork lays it out: the graph, the subject with what points
 * at the commit, the author, the date and the short id. The rows are a
 * window over the graph furca-desktop holds - a screenful in the DOM and a
 * screenful read - and the graph column is one canvas the height of the
 * view, redrawn as it scrolls.
 */
export function GraphView({ path, graph, labels, head, selected, onSelect, query, found, inMessage }: GraphViewProps) {
  const { t, i18n } = useTranslation()
  const viewport = useRef<HTMLDivElement>(null)
  const canvas = useRef<HTMLCanvasElement>(null)
  const [scrollTop, setScrollTop] = useState(0)
  const [height, setHeight] = useState(0)
  const palette = usePalette()
  const ids = useId()
  const ready = graph.state !== null

  // The view is there once the graph is: measured then, and watched after.
  useEffect(() => {
    const element = viewport.current
    if (!element) return
    const measure = () => {
      setHeight(element.clientHeight)
      setScrollTop(element.scrollTop)
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(element)
    return () => observer.disconnect()
  }, [ready])

  const count = graph.state?.rows ?? 0
  const body = Math.max(0, height - HEADER)
  const { start, end, totalHeight, offsetTop } = windowFor({
    count,
    rowHeight: GEOMETRY.row,
    viewportHeight: body,
    scrollTop,
    overscan: OVERSCAN,
  })
  const graphWidth = columnWidth(graph.state?.width ?? 1)
  const columns = `${String(graphWidth)}px minmax(0, 1fr) 10rem 9.5rem 5rem`

  useEffect(() => wantRows(path, start, end), [path, start, end, count, graph.version])
  const rows = useMemo(
    () => Array.from({ length: end - start }, (_, offset) => rowAt(path, start + offset)),
    // `version` is what says a page arrived.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [path, start, end, graph.version],
  )

  // Drawn after the rows are in the DOM and before the frame is painted, so
  // the lines and the text scroll as one.
  useLayoutEffect(() => {
    const element = canvas.current
    if (!element) return
    const ratio = window.devicePixelRatio || 1
    const width = Math.round(graphWidth * ratio)
    const tall = Math.round(body * ratio)
    if (element.width !== width) element.width = width
    if (element.height !== tall) element.height = tall
    const context = element.getContext('2d')
    if (!context) return
    context.setTransform(ratio, 0, 0, ratio, 0, 0)
    context.clearRect(0, 0, graphWidth, body)
    draw(context, rows, start * GEOMETRY.row - scrollTop, palette, head)
  }, [rows, start, scrollTop, body, graphWidth, palette, head])

  // The chosen row is kept in view: moved to by the keyboard, it scrolls
  // just enough; jumped to from a search or a branch, it lands a third of
  // the way down, where the rows around it can be seen.
  useEffect(() => {
    const element = viewport.current
    if (!element || selected === null || body === 0) return
    const top = selected * GEOMETRY.row
    const shown = element.scrollTop
    if (top >= shown && top + GEOMETRY.row <= shown + body) return
    const near = top >= shown - GEOMETRY.row * 2 && top <= shown + body + GEOMETRY.row
    element.scrollTop = near
      ? top < shown
        ? top
        : top + GEOMETRY.row - body
      : Math.max(0, top - Math.floor(body / 3))
  }, [selected, body])

  const date = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' }),
    [i18n.language],
  )

  if (graph.failed !== null && graph.state === null) {
    return (
      <div className="p-6">
        <Alert tone="bad" title={t('graph.failed')}>
          {graph.failed}
        </Alert>
      </div>
    )
  }
  if (graph.state === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('graph.reading')} />
      </div>
    )
  }
  if (graph.state.complete && count === 0) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <EmptyState title={t('graph.empty')} body={t('graph.emptyBody')} />
      </div>
    )
  }

  const move = (event: KeyboardEvent<HTMLDivElement>) => {
    const page = Math.max(1, Math.floor(body / GEOMETRY.row) - 1)
    const from = selected ?? -1
    const to =
      event.key === 'ArrowDown'
        ? from + 1
        : event.key === 'ArrowUp'
          ? from - 1
          : event.key === 'PageDown'
            ? from + page
            : event.key === 'PageUp'
              ? from - page
              : event.key === 'Home'
                ? 0
                : event.key === 'End'
                  ? count - 1
                  : null
    if (to === null || count === 0) return
    event.preventDefault()
    onSelect(Math.min(Math.max(to, 0), count - 1))
  }

  const rowId = (index: number) => `${ids}-row-${String(index)}`
  const active = selected !== null && selected >= start && selected < end ? rowId(selected) : undefined

  return (
    <div className="relative h-full min-h-0">
      <div
        ref={viewport}
        role="grid"
        aria-label={t('graph.label')}
        aria-rowcount={count + 1}
        aria-colcount={5}
        aria-activedescendant={active}
        tabIndex={0}
        onKeyDown={move}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        className={cn(
          'absolute inset-0 overflow-y-auto overflow-x-hidden',
          'focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent',
        )}
      >
        <div
          role="row"
          aria-rowindex={1}
          className="sticky top-0 z-10 grid items-center border-b border-line bg-raise text-xs text-dim"
          style={{ height: HEADER, gridTemplateColumns: columns }}
        >
          <Header>{t('graph.columns.graph')}</Header>
          <Header>{t('graph.columns.subject')}</Header>
          <Header>{t('graph.columns.author')}</Header>
          <Header>{t('graph.columns.date')}</Header>
          <Header>{t('graph.columns.commit')}</Header>
        </div>
        <div style={{ height: totalHeight }} className="relative">
          <div style={{ transform: `translateY(${String(offsetTop)}px)` }} className="absolute inset-x-0 top-0">
            {rows.map((row, offset) => {
              const index = start + offset
              return (
                <GraphLine
                  key={index}
                  id={rowId(index)}
                  index={index}
                  row={row}
                  columns={columns}
                  labels={row ? labels.get(row.commit.id) : undefined}
                  selected={index === selected}
                  query={found.has(index) ? query : ''}
                  inMessage={inMessage.has(index)}
                  date={date}
                  onSelect={() => {
                    onSelect(index)
                    viewport.current?.focus()
                  }}
                />
              )
            })}
          </div>
        </div>
      </div>
      <canvas
        ref={canvas}
        aria-hidden
        className="pointer-events-none absolute left-0"
        style={{ top: HEADER, width: graphWidth, height: body }}
      />
    </div>
  )
}

function Header({ children }: { children: ReactNode }) {
  return (
    <div role="columnheader" className="truncate px-2 font-medium">
      {children}
    </div>
  )
}

function GraphLine({
  id,
  index,
  row,
  columns,
  labels,
  selected,
  query,
  inMessage,
  date,
  onSelect,
}: {
  id: string
  index: number
  row: GraphRow | undefined
  columns: string
  labels: Label[] | undefined
  selected: boolean
  /** The search's words, on a row the search found; empty otherwise. */
  query: string
  inMessage: boolean
  date: Intl.DateTimeFormat
  onSelect: () => void
}) {
  const { t } = useTranslation()
  const commit = row?.commit
  return (
    <div
      id={id}
      role="row"
      aria-rowindex={index + 2}
      aria-selected={selected}
      onMouseDown={(event) => {
        if (event.button === 0) onSelect()
      }}
      className={cn(
        'grid cursor-default items-center text-sm',
        selected ? 'bg-soft' : 'hover:bg-softer',
      )}
      style={{ height: GEOMETRY.row, gridTemplateColumns: columns }}
    >
      <div role="gridcell" />
      <div role="gridcell" className="flex min-w-0 items-center gap-1 px-2">
        {labels?.map((label) => <RefLabel key={`${label.kind}:${label.name}`} label={label} />)}
        <span className={cn('truncate', !commit && 'text-faint')} title={commit?.subject}>
          <Marked text={commit?.subject ?? ''} query={query} />
        </span>
        {inMessage && (
          <span className="shrink-0 text-2xs text-dim" title={t('graph.inMessageHint')}>
            {t('graph.inMessage')}
          </span>
        )}
      </div>
      <div role="gridcell" className="truncate px-2 text-dim" title={commit?.author.email}>
        <Marked text={commit?.author.name ?? ''} query={query} />
      </div>
      <div role="gridcell" className="truncate px-2 text-dim" title={commit?.author.time}>
        {commit ? date.format(new Date(commit.author.seconds * 1000)) : ''}
      </div>
      <div role="gridcell" className="px-2 font-mono text-xs text-dim">
        <Marked text={commit ? short(commit.id) : ''} query={startOfId(query)} />
      </div>
    </div>
  )
}

/** The search's words as the start of an id, when they can be one - as the
 * search itself takes them: four hex digits or more. */
function startOfId(query: string): string {
  const words = query.trim()
  return /^[0-9a-fA-F]{4,}$/.test(words) ? words : ''
}

/** `text` with the places a search found marked, as nooma marks the words
 * it matched. */
function Marked({ text, query }: { text: string; query: string }) {
  if (query === '') return text
  return markedPieces(text, query).map((piece, index) =>
    piece.marked ? (
      <mark key={index} className="border-b border-info bg-info-soft px-px text-text">
        {piece.text}
      </mark>
    ) : (
      <span key={index}>{piece.text}</span>
    ),
  )
}

function RefLabel({ label }: { label: Label }) {
  return (
    <span
      className={cn(
        'inline-flex h-4 max-w-48 shrink-0 items-center gap-0.5 rounded-sm border px-1 text-2xs leading-none',
        label.kind === 'head' && 'border-accent bg-accent text-on-accent',
        label.kind === 'branch' && 'border-line-2 text-text',
        label.kind === 'remote' && 'border-line text-dim',
        (label.kind === 'tag' || label.kind === 'stash') && 'border-line text-dim',
      )}
      title={label.name}
    >
      {label.kind === 'tag' && <TagIcon aria-hidden className="size-2.5 shrink-0" />}
      {label.kind === 'stash' && <Archive aria-hidden className="size-2.5 shrink-0" />}
      <span className="truncate">{label.name}</span>
    </span>
  )
}
