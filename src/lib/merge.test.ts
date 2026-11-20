import { describe, expect, it } from 'vitest'
import type { Block } from './generated/Block'
import type { Conflict } from './generated/Conflict'
import { lineStarts, linesOf, markersIn, mergedName, offsets, pickedText, shownTexts, startingResult, withoutCr } from './merge'

const range = (start: number, count: number) => ({ start, count })
const block = (kind: Block['kind'], base: [number, number], ours: [number, number], theirs: [number, number]): Block => ({
  kind,
  base: range(...base),
  ours: range(...ours),
  theirs: range(...theirs),
})

/** The conflict the core reads for these texts (crates/furca-core/tests/merges.rs). */
function conflict(): Conflict {
  return {
    path: 'f.txt',
    kind: 'both-modified',
    base: null,
    ours: null,
    theirs: null,
    texts: {
      base: 'one\ntwo\nthree\nfour\nfive\n',
      ours: 'one\n2 from main\nthree\nfour\nFIVE\n',
      theirs: 'one\nTWO from feature\nthree\nfour\nfive\n',
    },
    blocks: [
      block('unchanged', [0, 1], [0, 1], [0, 1]),
      block('conflict', [1, 1], [1, 1], [1, 1]),
      block('unchanged', [2, 2], [2, 2], [2, 2]),
      block('ours', [4, 1], [4, 1], [4, 1]),
    ],
    marked: 2,
  }
}

const labels = { ours: 'main', theirs: 'feature' }

describe('the resolver result', () => {
  it('starts with what goes in on its own in, and each conflict between its markers', () => {
    const c = conflict()
    const shown = shownTexts(c)!
    const { text, spans } = startingResult(c, shown, labels)
    expect(text).toBe(
      'one\n<<<<<<< main\n2 from main\n||||||| base\ntwo\n=======\nTWO from feature\n>>>>>>> feature\nthree\nfour\nFIVE\n',
    )
    expect(spans).toHaveLength(1)
    const span = spans[0]!
    expect(text.slice(span.from, span.to)).toMatch(/^<<<<<<< main\n[\s\S]*>>>>>>> feature\n$/)
    expect(markersIn(text)).toBe(2)
  })

  it('takes a side for a conflict', () => {
    const c = conflict()
    const shown = shownTexts(c)!
    expect(pickedText(c, shown, 0, 'ours')).toBe('2 from main\n')
    expect(pickedText(c, shown, 0, 'theirs')).toBe('TWO from feature\n')
    expect(pickedText(c, shown, 0, 'both')).toBe('2 from main\nTWO from feature\n')
    expect(pickedText(c, shown, 0, 'base')).toBe('two\n')
    expect(pickedText(c, shown, 3, 'base')).toBe('')
  })

  it('keeps a last line with no newline from running into a marker', () => {
    const c: Conflict = {
      ...conflict(),
      texts: { base: 'a\nb', ours: 'a\nmine', theirs: 'a\ntheirs' },
      blocks: [block('unchanged', [0, 1], [0, 1], [0, 1]), block('conflict', [1, 1], [1, 1], [1, 1])],
    }
    const { text } = startingResult(c, shownTexts(c)!, labels)
    expect(text).toBe('a\n<<<<<<< main\nmine\n||||||| base\nb\n=======\ntheirs\n>>>>>>> feature\n')
  })

  it('shows lines without the carriage return the core gives back', () => {
    expect(withoutCr('a\r\nb\r\nc\r')).toBe('a\nb\nc')
    expect(withoutCr('a\rb\n')).toBe('a\rb\n')
  })
})

describe('markers', () => {
  it('are found where a conflict is still written out, and not under a heading', () => {
    expect(markersIn('a\n<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> b\n')).toBe(2)
    expect(markersIn('Title\n=======\n\ntext\n')).toBeNull()
    expect(markersIn('<<<<<<<< eight\n>>>>>>>> eight\n')).toBeNull()
    expect(markersIn('<<<<<<< opened and never closed\n')).toBeNull()
  })
})

describe('lines and offsets', () => {
  it('cuts a text at its lines and finds a run of them', () => {
    expect(linesOf('a\nb')).toEqual(['a\n', 'b'])
    expect(linesOf('')).toEqual([])
    const starts = lineStarts('one\ntwo\nthree\n')
    expect(starts).toEqual([0, 4, 8, 14])
    expect(offsets(starts, range(1, 1))).toEqual({ from: 4, to: 8 })
    expect(offsets(starts, range(3, 0))).toEqual({ from: 14, to: 14 })
  })

  it('reads the name a merge message gives what it merges', () => {
    expect(mergedName("Merge branch 'feature/login' into main")).toBe('feature/login')
    expect(mergedName("Merge remote-tracking branch 'origin/x'")).toBe('origin/x')
    expect(mergedName('Merge commit abc')).toBeNull()
    expect(mergedName(null)).toBeNull()
  })
})
