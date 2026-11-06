import { isBranch, type TreeRow } from '@/components/ui/tree-rows'
import { windowFor } from '@/components/ui/virtual-list'

/*
 * The sums behind a long tree of files, with no React in them: which rows to
 * draw, where a row stands among its neighbours, what a key does and how far to
 * scroll for a row to show. `src/repo/FileTree.tsx` is what draws them.
 */

/** How tall a row is: the 16px line of `text-sm` and 4px above and below it,
 * as dowel's TreeView draws its rows. One height is what lets a row's place be
 * worked out without drawing the rows before it. */
export const ROW_HEIGHT = 24
/** The room around the rows, inside the box that scrolls. */
export const TREE_PADDING = 6

/** Where a row stands among the rows of its folder, as `aria-setsize` and
 * `aria-posinset` say it. A reader works this out from the DOM, and the DOM
 * holds a screenful. */
export interface Place {
  size: number
  at: number
}

/** The place of every row. A row is shown only under an open folder, so the
 * rows of one folder are either all here or all not. */
export function placesOf(rows: readonly TreeRow[]): Place[] {
  const sizes = new Map<string | undefined, number>()
  for (const row of rows) sizes.set(row.parent, (sizes.get(row.parent) ?? 0) + 1)
  const reached = new Map<string | undefined, number>()
  return rows.map((row) => {
    const at = (reached.get(row.parent) ?? 0) + 1
    reached.set(row.parent, at)
    return { size: sizes.get(row.parent) ?? at, at }
  })
}

/** The row the keyboard is on: the one it was moved to while that row is shown,
 * then the one chosen, then the first; -1 when there are no rows. */
export function activeRow(indexes: ReadonlyMap<string, number>, cursor: string | undefined, selected: string | undefined): number {
  const indexOf = (id: string | undefined) => (id === undefined ? undefined : indexes.get(id))
  return indexOf(cursor) ?? indexOf(selected) ?? (indexes.size > 0 ? 0 : -1)
}

/** The rows to draw: the ones in view and a few beyond each edge, and the row
 * the keyboard is on wherever it is, because `aria-activedescendant` has to name
 * something in the DOM after the wheel has scrolled away from it. */
export function rowsToDraw(count: number, viewportHeight: number, scrollTop: number, active: number): number[] {
  const { start, end } = windowFor({ count, rowHeight: ROW_HEIGHT, viewportHeight, scrollTop: scrollTop - TREE_PADDING })
  const rows: number[] = []
  for (let index = start; index < end; index += 1) rows.push(index)
  if (active >= 0 && active < count && (active < start || active >= end)) rows.push(active)
  return rows
}

/** Where the box has to be scrolled to for a row to show with the room around
 * it, moving as little as it can: `scrollTop` itself when the row shows already.
 * A box too short for a row shows the row's top. */
export function scrollToShow(index: number, scrollTop: number, viewportHeight: number): number {
  const from = index * ROW_HEIGHT
  const to = from + ROW_HEIGHT + 2 * TREE_PADDING
  if (from < scrollTop || to - from > viewportHeight) return from
  if (to > scrollTop + viewportHeight) return to - viewportHeight
  return scrollTop
}

/** What a key does to a tree: the cursor moves, a folder opens or shuts, a file
 * is chosen. `nothing` is a key of the tree's with nothing to do where it is. */
export type TreeAction =
  | { kind: 'cursor'; id: string }
  | { kind: 'open'; id: string; open: boolean }
  | { kind: 'choose'; id: string }
  | { kind: 'nothing' }

/**
 * What `key` does with the keyboard on row `index`; `null` for a key that is not
 * the tree's, which is left to whatever else wants it.
 *
 * Right opens a folder and then steps into it, as two presses, because looking
 * inside and moving inside are different intents. Left shuts an open folder and
 * otherwise goes to the row's folder in one press, out of a deep branch without
 * walking back past every neighbour. The same keys as dowel's TreeView.
 */
export function actionFor(key: string, rows: readonly TreeRow[], index: number, open: ReadonlySet<string>): TreeAction | null {
  const current = rows[index]
  if (current === undefined) return null
  const { id } = current.node
  const branch = isBranch(current.node)
  const isOpen = open.has(id)
  const cursorAt = (to: number): TreeAction => {
    const row = rows[Math.min(Math.max(0, to), rows.length - 1)]
    return row === undefined ? { kind: 'nothing' } : { kind: 'cursor', id: row.node.id }
  }
  switch (key) {
    case 'ArrowDown':
      return cursorAt(index + 1)
    case 'ArrowUp':
      return cursorAt(index - 1)
    case 'ArrowRight':
      if (!branch) return { kind: 'nothing' }
      return isOpen ? cursorAt(index + 1) : { kind: 'open', id, open: true }
    case 'ArrowLeft':
      if (branch && isOpen) return { kind: 'open', id, open: false }
      return current.parent === undefined ? { kind: 'nothing' } : { kind: 'cursor', id: current.parent }
    case 'Home':
      return cursorAt(0)
    case 'End':
      return cursorAt(rows.length - 1)
    case 'Enter':
    case ' ':
      return branch ? { kind: 'open', id, open: !isOpen } : { kind: 'choose', id }
    default:
      return null
  }
}
