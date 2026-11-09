import { useId, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { cn } from 'dowel-ui'
import { ScrollArea } from '@/components/ui/scroll-area'
import { isBranch, visibleRows, type TreeNode } from '@/components/ui/tree-rows'
import { actionFor, activeRow, placesOf, ROW_HEIGHT, rowsToDraw, scrollToShow, TREE_PADDING } from '@/lib/file-tree'
import { useScrollBox } from './useScrollBox'

export interface FileTreeProps {
  /** What a screen reader calls the tree and the box it scrolls in. */
  label: string
  nodes: readonly TreeNode[]
  /** Which folders are open. */
  open: ReadonlySet<string>
  onOpenChange: (open: Set<string>) => void
  /** The row chosen, which is not always the one the keyboard is on: a reader
   * walks the tree with the arrows and chooses with Enter. */
  selected?: string
  onSelect: (id: string) => void
  /** Space on a row, when the rows can be acted on: where Enter chooses, this
   * does what the row is for. */
  onAct?: (id: string) => void
  /** Delete on a row, when what it stands for can be thrown away. */
  onDiscard?: (id: string) => void
}

/**
 * A tree of changed files that puts a screenful of rows in the DOM, however many
 * files there are: a commit of ten thousand is otherwise ten thousand rows laid
 * out again on every change, even behind a hidden graph. It behaves as dowel's
 * TreeView does, one tab stop with a cursor inside; `VirtualList` is not under
 * it because its `list` and `listitem` roles would be wrong in a `tree`, so it
 * shares the arithmetic instead (`src/lib/file-tree.ts`).
 */
export function FileTree({ label, nodes, open, onOpenChange, selected, onSelect, onAct, onDiscard }: FileTreeProps) {
  const base = useId()
  const { viewport, scrollTop, viewportHeight, scrollTo } = useScrollBox()
  const [cursor, setCursor] = useState<string | undefined>(undefined)

  // A row chosen from outside takes the cursor with it, so the arrows go on from
  // what is open. Chosen here, by click or Enter, the cursor is on it already.
  const [chosen, setChosen] = useState(selected)
  if (chosen !== selected) {
    setChosen(selected)
    setCursor(undefined)
  }

  const rows = useMemo(() => visibleRows(nodes, open), [nodes, open])
  const indexes = useMemo(() => new Map(rows.map((row, index) => [row.node.id, index])), [rows])
  const places = useMemo(() => placesOf(rows), [rows])
  const active = activeRow(indexes, cursor, selected)

  const reveal = (index: number) => {
    const element = viewport.current
    if (element !== null) scrollTo(scrollToShow(index, element.scrollTop, element.clientHeight))
  }

  // A row chosen from outside - the file a commit was left on, the next one
  // after staging - is brought into view once. A tree behind a hidden graph has
  // no height to bring it into, so it waits until it has one. A tree scrolled
  // by hand stays where it was put until the choice changes again.
  const toReveal = useRef(true)
  useLayoutEffect(() => {
    toReveal.current = true
  }, [selected])
  useLayoutEffect(() => {
    if (!toReveal.current || viewportHeight === 0) return
    toReveal.current = false
    const index = selected === undefined ? undefined : indexes.get(selected)
    if (index !== undefined) reveal(index)
  })

  const openFolder = (id: string, next: boolean) => {
    const copy = new Set(open)
    if (next) copy.add(id)
    else copy.delete(id)
    onOpenChange(copy)
  }

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === ' ' && onAct !== undefined) {
      const row = rows[active]
      if (row !== undefined) {
        event.preventDefault()
        onAct(row.node.id)
      }
      return
    }
    if (event.key === 'Delete' && onDiscard !== undefined) {
      const row = rows[active]
      if (row !== undefined) {
        event.preventDefault()
        onDiscard(row.node.id)
      }
      return
    }
    const action = actionFor(event.key, rows, active, open)
    if (action === null) return
    event.preventDefault()
    if (action.kind === 'cursor') {
      setCursor(action.id)
      const index = indexes.get(action.id)
      if (index !== undefined) reveal(index)
    } else if (action.kind === 'open') {
      openFolder(action.id, action.open)
    } else if (action.kind === 'choose') {
      onSelect(action.id)
    }
  }

  return (
    <ScrollArea label={label} orientation="vertical" className="h-full" viewportRef={viewport}>
      <div
        role="tree"
        aria-label={label}
        // One stop for the whole tree; the arrows move the cursor inside it.
        tabIndex={0}
        onKeyDown={onKeyDown}
        aria-activedescendant={active === -1 ? undefined : `${base}-${String(active)}`}
        style={{ padding: TREE_PADDING }}
        className="text-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
      >
        {/* As tall as every row together: what the scrollbar measures. */}
        <div className="relative" style={{ height: rows.length * ROW_HEIGHT }}>
          {rowsToDraw(rows.length, viewportHeight, scrollTop, active).map((index) => {
            const row = rows[index]
            const place = places[index]
            if (row === undefined || place === undefined) return null
            const { node, depth } = row
            const branch = isBranch(node)
            const isOpen = open.has(node.id)
            return (
              <div
                key={node.id}
                // By the row's place rather than its id: a path can hold a
                // space, and `aria-activedescendant` takes a single word.
                id={`${base}-${String(index)}`}
                role="treeitem"
                aria-expanded={branch ? isOpen : undefined}
                aria-selected={node.id === selected || undefined}
                aria-level={depth + 1}
                aria-setsize={place.size}
                aria-posinset={place.at}
                onClick={() => {
                  setCursor(node.id)
                  if (branch) openFolder(node.id, !isOpen)
                  else onSelect(node.id)
                }}
                className={cn(
                  'group absolute inset-x-0 flex cursor-pointer items-center gap-1 rounded-sm pr-2 transition-colors',
                  node.id === selected ? 'bg-accent-soft text-text' : 'hover:bg-soft',
                  index === active && node.id !== selected && 'bg-softer',
                )}
                // Each at its own place, so a row the keyboard left off screen
                // can be drawn apart from the rows in view.
                style={{ top: index * ROW_HEIGHT, height: ROW_HEIGHT, paddingLeft: depth * 16 + 8 }}
              >
                <span aria-hidden className={cn('w-3 shrink-0 text-2xs text-dim', !branch && 'opacity-0')}>
                  {isOpen ? '▾' : '▸'}
                </span>
                <span className="min-w-0 flex-1 truncate">{node.label}</span>
              </div>
            )
          })}
        </div>
      </div>
    </ScrollArea>
  )
}
