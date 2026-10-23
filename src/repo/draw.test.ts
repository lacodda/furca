import { describe, expect, it } from 'vitest'
import type { Row } from '@/lib/generated/Row'
import { columnWidth, FEWEST_LANES, GEOMETRY, laneX, MOST_LANES, rowStrokes } from './draw'

const geometry = { row: 20, lane: 10, pad: 4, dot: 3 }

describe('rowStrokes', () => {
  it('runs a passing lane top to bottom, an ending one into the dot, a parent from the dot down', () => {
    const row: Row = {
      column: 0,
      color: 1,
      segments: [
        { kind: 'in', from: 0, to: 0, color: 1 },
        { kind: 'pass', from: 1, to: 1, color: 2 },
        { kind: 'in', from: 2, to: 0, color: 3 },
        { kind: 'out', from: 0, to: 0, color: 1 },
        { kind: 'out', from: 0, to: 3, color: 4 },
      ],
    }
    const x = (column: number) => laneX(column, geometry)
    expect(rowStrokes(row, 100, geometry)).toEqual([
      { color: 1, from: [x(0), 100], to: [x(0), 110] },
      { color: 2, from: [x(1), 100], to: [x(1), 120] },
      { color: 3, from: [x(2), 100], to: [x(0), 110] },
      { color: 1, from: [x(0), 110], to: [x(0), 120] },
      { color: 4, from: [x(0), 110], to: [x(3), 120] },
    ])
  })

  it('meets the next row where this one ends: a lane is one line down the column', () => {
    const above: Row = { column: 0, color: 0, segments: [{ kind: 'out', from: 0, to: 1, color: 5 }] }
    const below: Row = { column: 0, color: 0, segments: [{ kind: 'pass', from: 1, to: 1, color: 5 }] }
    const end = rowStrokes(above, 0, geometry)[0]!.to
    const start = rowStrokes(below, geometry.row, geometry)[0]!.from
    expect(start).toEqual(end)
  })
})

describe('columnWidth', () => {
  it('grows by a lane per lane, from the fewest it shows to the most', () => {
    expect(columnWidth(0)).toBe(columnWidth(FEWEST_LANES))
    expect(columnWidth(FEWEST_LANES + 1) - columnWidth(FEWEST_LANES)).toBe(GEOMETRY.lane)
    expect(columnWidth(MOST_LANES + 20)).toBe(columnWidth(MOST_LANES))
  })
})
