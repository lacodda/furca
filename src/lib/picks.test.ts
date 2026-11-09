import { describe, expect, it } from 'vitest'
import type { Hunk } from './generated/Hunk'
import { NO_PICKS, isChanged, pickCount, pickedOf, setLines, stillPicked, wholeHunk } from './picks'

/** Lines 2 and 3 replaced by two new ones, and one line added after line 9. */
const hunks: Hunk[] = [
  { before: { start: 2, count: 2 }, after: { start: 2, count: 2 } },
  { before: { start: 9, count: 0 }, after: { start: 9, count: 1 } },
]

describe('picking lines', () => {
  it('takes only the lines a hunk changed', () => {
    expect(isChanged(hunks, 'before', 2)).toBe(true)
    expect(isChanged(hunks, 'before', 4)).toBe(false)
    expect(isChanged(hunks, 'after', 9)).toBe(true)
    expect(isChanged(hunks, 'before', 9)).toBe(false)
    const picks = setLines(NO_PICKS, hunks, 'after', 0, 12, true)
    expect([...picks.after].sort((a, b) => a - b)).toEqual([2, 3, 9])
    expect(picks.before.size).toBe(0)
  })

  it('sets a range either way round, and lets it go again', () => {
    const picks = setLines(NO_PICKS, hunks, 'before', 3, 2, true)
    expect(pickCount(picks)).toBe(2)
    const less = setLines(picks, hunks, 'before', 3, 3, false)
    expect([...less.before]).toEqual([2])
    expect(pickCount(picks)).toBe(2)
  })

  it('goes to the core per hunk, in order, with nothing for an untouched hunk', () => {
    let picks = setLines(NO_PICKS, hunks, 'after', 9, 9, true)
    picks = setLines(picks, hunks, 'before', 3, 3, true)
    picks = setLines(picks, hunks, 'after', 2, 2, true)
    expect(pickedOf(picks, hunks)).toEqual([
      { hunk: hunks[0], removed: [3], added: [2] },
      { hunk: hunks[1], removed: [], added: [9] },
    ])
    expect(pickedOf(NO_PICKS, hunks)).toEqual([])
  })

  it('lets go of a pick the file no longer changes, and keeps the same picks otherwise', () => {
    const picks = setLines(setLines(NO_PICKS, hunks, 'after', 9, 9, true), hunks, 'before', 2, 2, true)
    expect(stillPicked(picks, hunks)).toBe(picks)
    const moved = stillPicked(picks, hunks.slice(0, 1))
    expect([...moved.after]).toEqual([])
    expect([...moved.before]).toEqual([2])
  })

  it('takes a whole hunk as every line of both its sides', () => {
    expect(wholeHunk(hunks[0]!)).toEqual({ hunk: hunks[0], removed: [2, 3], added: [2, 3] })
    expect(wholeHunk(hunks[1]!)).toEqual({ hunk: hunks[1], removed: [], added: [9] })
  })
})
