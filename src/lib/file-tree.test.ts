import { describe, expect, it } from 'vitest'
import { visibleRows, type TreeNode, type TreeRow } from '@/components/ui/tree-rows'
import { actionFor, activeRow, placesOf, ROW_HEIGHT, rowsToDraw, scrollToShow, TREE_PADDING } from './file-tree'

const leaf = (id: string): TreeNode => ({ id, label: id })
const folder = (id: string, children: TreeNode[]): TreeNode => ({ id, label: id, children })

/** a/ holding b/ (b1, b2) and a1; c; an empty folder e. */
const nodes: TreeNode[] = [folder('a', [folder('b', [leaf('b1'), leaf('b2')]), leaf('a1')]), leaf('c'), { id: 'e', label: 'e', empty: true }]
const open = new Set(['a', 'b'])
const rows: TreeRow[] = visibleRows(nodes, open)
const ids = rows.map((row) => row.node.id)
const at = (id: string) => ids.indexOf(id)

describe('the rows to draw', () => {
  const screen = 480
  const inView = Math.ceil(screen / ROW_HEIGHT)

  it('are a screenful, however many rows there are', () => {
    for (const count of [10_000, 1_000_000]) {
      const drawn = rowsToDraw(count, screen, 0, 0)
      expect(drawn.length).toBeGreaterThanOrEqual(inView)
      expect(drawn.length).toBeLessThanOrEqual(inView + 7)
    }
  })

  it('follow the scroll, covering every row the box shows', () => {
    const scrollTop = 5_000 * ROW_HEIGHT + TREE_PADDING + 7
    const drawn = rowsToDraw(10_000, screen, scrollTop, 5_000)
    const first = Math.floor((scrollTop - TREE_PADDING) / ROW_HEIGHT)
    const last = Math.floor((scrollTop - TREE_PADDING + screen) / ROW_HEIGHT)
    for (let row = first; row <= last; row += 1) expect(drawn).toContain(row)
    expect(drawn).not.toContain(0)
    expect(drawn).not.toContain(9_999)
  })

  it('take the padding above the first row into account', () => {
    // One pixel of row 3 still shows at the top edge, so three rows are drawn
    // above it; a pixel further and row 4 is the first that shows.
    const lastPixelOfRow3 = TREE_PADDING + 4 * ROW_HEIGHT - 1
    expect(rowsToDraw(10_000, ROW_HEIGHT, lastPixelOfRow3, 4)[0]).toBe(0)
    const drawn = rowsToDraw(10_000, ROW_HEIGHT, lastPixelOfRow3 + 1, 4)
    expect(drawn[0]).toBe(1)
    expect(drawn.at(-1)).toBe(8)
  })

  it('hold the row the keyboard is on, once, wherever it is', () => {
    const below = rowsToDraw(10_000, screen, 0, 9_000)
    expect(below.filter((row) => row === 9_000)).toHaveLength(1)
    expect(below.at(-1)).toBe(9_000)
    const above = rowsToDraw(10_000, screen, 5_000 * ROW_HEIGHT, 3)
    expect(above.filter((row) => row === 3)).toHaveLength(1)
    const inside = rowsToDraw(10_000, screen, 0, 2)
    expect(inside.filter((row) => row === 2)).toHaveLength(1)
  })

  it('are none of no rows, and nothing past the end', () => {
    expect(rowsToDraw(0, screen, 0, -1)).toEqual([])
    expect(rowsToDraw(3, screen, 0, 7)).toEqual([0, 1, 2])
    expect(rowsToDraw(3, screen, 0, -1)).toEqual([0, 1, 2])
  })
})

describe('scrolling a row into view', () => {
  const viewport = 240
  const room = ROW_HEIGHT + 2 * TREE_PADDING

  it('leaves the box alone when the row shows', () => {
    expect(scrollToShow(0, 0, viewport)).toBe(0)
    expect(scrollToShow(5, 0, viewport)).toBe(0)
    // The rows that just fit, with the room below the last of them.
    const scrollTop = 1_000 * ROW_HEIGHT
    expect(scrollToShow(1_000, scrollTop, viewport)).toBe(scrollTop)
    const lastThatFits = 1_000 + Math.floor((viewport - room) / ROW_HEIGHT)
    expect(scrollToShow(lastThatFits, scrollTop, viewport)).toBe(scrollTop)
  })

  it('goes down as little as it can for a row below', () => {
    const row = 100
    expect(scrollToShow(row, 0, viewport)).toBe(row * ROW_HEIGHT + room - viewport)
    // One row past the edge is one row of scrolling.
    const scrollTop = 1_000 * ROW_HEIGHT
    const firstPast = 1_000 + Math.floor((viewport - room) / ROW_HEIGHT) + 1
    const moved = scrollToShow(firstPast, scrollTop, viewport)
    expect(moved).toBeGreaterThan(scrollTop)
    expect(moved - scrollTop).toBeLessThanOrEqual(ROW_HEIGHT)
  })

  it('goes up to the row for a row above', () => {
    expect(scrollToShow(3, 1_000, viewport)).toBe(3 * ROW_HEIGHT)
  })

  it('shows the padding around the first and the last row', () => {
    expect(scrollToShow(0, 100, viewport)).toBe(0)
    const count = 500
    const whole = count * ROW_HEIGHT + 2 * TREE_PADDING
    expect(scrollToShow(count - 1, 0, viewport)).toBe(whole - viewport)
  })

  it("shows a row's top in a box too short for it", () => {
    expect(scrollToShow(4, 0, 10)).toBe(4 * ROW_HEIGHT)
    expect(scrollToShow(4, 4 * ROW_HEIGHT + 3, 10)).toBe(4 * ROW_HEIGHT)
  })
})

describe('where a row stands among its neighbours', () => {
  const places = placesOf(rows)
  const place = (id: string) => places[at(id)]

  it('counts the rows of its own folder, not of the tree', () => {
    expect(place('a')).toEqual({ size: 3, at: 1 })
    expect(place('b')).toEqual({ size: 2, at: 1 })
    expect(place('b1')).toEqual({ size: 2, at: 1 })
    expect(place('b2')).toEqual({ size: 2, at: 2 })
    expect(place('a1')).toEqual({ size: 2, at: 2 })
    expect(place('c')).toEqual({ size: 3, at: 2 })
    expect(place('e')).toEqual({ size: 3, at: 3 })
  })

  it('is one for every row', () => {
    expect(places).toHaveLength(rows.length)
    expect(placesOf([])).toEqual([])
  })
})

describe('the row the keyboard is on', () => {
  const indexes = new Map(ids.map((id, index) => [id, index]))

  it('is where it was moved to, before what is chosen', () => {
    expect(activeRow(indexes, 'c', 'b1')).toBe(at('c'))
  })

  it('falls back on what is chosen once its row has gone', () => {
    expect(activeRow(indexes, 'gone', 'b1')).toBe(at('b1'))
    expect(activeRow(indexes, undefined, 'b1')).toBe(at('b1'))
  })

  it('falls back on the first row, and on none when there are none', () => {
    expect(activeRow(indexes, 'gone', 'gone too')).toBe(0)
    expect(activeRow(indexes, undefined, undefined)).toBe(0)
    expect(activeRow(new Map(), undefined, undefined)).toBe(-1)
  })
})

describe('what a key does', () => {
  const press = (key: string, id: string, opened: ReadonlySet<string> = open, shown: TreeRow[] = rows) =>
    actionFor(key, shown, shown.findIndex((row) => row.node.id === id), opened)
  const cursor = (id: string) => ({ kind: 'cursor', id })

  it('moves down and up a row, and stops at the ends', () => {
    expect(press('ArrowDown', 'b1')).toEqual(cursor('b2'))
    expect(press('ArrowUp', 'b1')).toEqual(cursor('b'))
    expect(press('ArrowDown', 'e')).toEqual(cursor('e'))
    expect(press('ArrowUp', 'a')).toEqual(cursor('a'))
  })

  it('goes to the first and the last row', () => {
    expect(press('Home', 'b1')).toEqual(cursor('a'))
    expect(press('End', 'b1')).toEqual(cursor('e'))
  })

  it('opens a closed folder on Right, and steps into an open one', () => {
    const shut = new Set(['a'])
    expect(press('ArrowRight', 'b', shut, visibleRows(nodes, shut))).toEqual({ kind: 'open', id: 'b', open: true })
    expect(press('ArrowRight', 'b')).toEqual(cursor('b1'))
    expect(press('ArrowRight', 'b1')).toEqual({ kind: 'nothing' })
  })

  it('shuts an open folder on Left, and goes to the folder otherwise', () => {
    expect(press('ArrowLeft', 'b')).toEqual({ kind: 'open', id: 'b', open: false })
    expect(press('ArrowLeft', 'b2')).toEqual(cursor('b'))
    const shut = new Set(['a'])
    expect(press('ArrowLeft', 'b', shut, visibleRows(nodes, shut))).toEqual(cursor('a'))
    expect(press('ArrowLeft', 'c')).toEqual({ kind: 'nothing' })
  })

  it('toggles a folder and chooses a file on Enter and Space', () => {
    for (const key of ['Enter', ' ']) {
      expect(press(key, 'b')).toEqual({ kind: 'open', id: 'b', open: false })
      expect(press(key, 'e')).toEqual({ kind: 'open', id: 'e', open: true })
      expect(press(key, 'b1')).toEqual({ kind: 'choose', id: 'b1' })
    }
  })

  it('leaves other keys alone, and every key where there are no rows', () => {
    expect(press('a', 'b1')).toBeNull()
    expect(press('Tab', 'b1')).toBeNull()
    expect(press('PageDown', 'b1')).toBeNull()
    expect(actionFor('ArrowDown', [], -1, open)).toBeNull()
  })
})
