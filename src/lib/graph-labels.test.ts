import { describe, expect, it } from 'vitest'
import type { Sidebar } from './generated/Sidebar'
import { commitOfColumnRow, labelsByCommit, markedPieces, matchNumber, nextMatch } from './graph-labels'

const sidebar = (over: Partial<Sidebar> = {}): Sidebar => ({
  head: { branch: 'main', commit: 'c1', detached: false, upstream: 'origin/main' },
  refs: {
    branches: [
      { name: 'feature/x', target: 'c2', upstream: null, head: false },
      { name: 'main', target: 'c1', upstream: null, head: true },
    ],
    remote_branches: [{ name: 'origin/main', remote: 'origin', target: 'c1' }],
    tags: [
      { name: 'v0.9.0', target: 'c1', annotated: false },
      { name: 'v0.10.0', target: 'c1', annotated: true },
    ],
  },
  remotes: [],
  stashes: [{ index: 0, message: 'WIP on main', commit: 's0', time: '', seconds: 0 }],
  submodules: [],
  ...over,
})

describe('labelsByCommit', () => {
  it('puts the branch HEAD is on first, then branches, remotes, tags newest first', () => {
    const labels = labelsByCommit(sidebar())
    expect(labels.get('c1')).toEqual([
      { name: 'main', kind: 'head' },
      { name: 'origin/main', kind: 'remote' },
      { name: 'v0.10.0', kind: 'tag' },
      { name: 'v0.9.0', kind: 'tag' },
    ])
    expect(labels.get('c2')).toEqual([{ name: 'feature/x', kind: 'branch' }])
    expect(labels.get('s0')).toEqual([{ name: 'stash@{0}', kind: 'stash' }])
  })

  it('names a detached HEAD where it is', () => {
    const labels = labelsByCommit(
      sidebar({ head: { branch: null, commit: 'c2', detached: true, upstream: null } }),
    )
    expect(labels.get('c2')?.[0]).toEqual({ name: 'HEAD', kind: 'head' })
  })
})

describe('nextMatch', () => {
  const matches = [3, 10, 42]

  it('moves to the next match below the chosen row, and round to the top', () => {
    expect(nextMatch(matches, 3, false)).toBe(10)
    expect(nextMatch(matches, 5, false)).toBe(10)
    expect(nextMatch(matches, 42, false)).toBe(3)
    expect(nextMatch(matches, null, false)).toBe(3)
  })

  it('moves to the one above going back, and round to the bottom', () => {
    expect(nextMatch(matches, 10, true)).toBe(3)
    expect(nextMatch(matches, 11, true)).toBe(10)
    expect(nextMatch(matches, 3, true)).toBe(42)
    expect(nextMatch(matches, null, true)).toBe(42)
  })

  it('has nowhere to go without matches', () => {
    expect(nextMatch([], 5, false)).toBeNull()
    expect(nextMatch([], null, true)).toBeNull()
  })
})

describe('matchNumber', () => {
  it('counts the chosen match from one, and none as zero', () => {
    const matches = [3, 10, 42, 77, 100]
    expect(matches.map((row) => matchNumber(matches, row))).toEqual([1, 2, 3, 4, 5])
    expect(matchNumber(matches, 11)).toBe(0)
    expect(matchNumber(matches, null)).toBe(0)
    expect(matchNumber([], 3)).toBe(0)
  })
})

describe('commitOfColumnRow', () => {
  it('finds the commit of a branch, a remote branch, a tag and a stash entry', () => {
    const data = sidebar()
    expect(commitOfColumnRow(data, 'b:feature/x')).toBe('c2')
    expect(commitOfColumnRow(data, 'r:origin:main')).toBe('c1')
    expect(commitOfColumnRow(data, 't:v0.9.0')).toBe('c1')
    expect(commitOfColumnRow(data, 's:0')).toBe('s0')
  })

  it('finds none for a folder, a section or a remote', () => {
    const data = sidebar()
    for (const id of ['branches', 'b/feature', 'r:origin', 'r:origin/feature', 'm:lib', 'b:gone']) {
      expect(commitOfColumnRow(data, id)).toBeNull()
    }
  })
})

describe('markedPieces', () => {
  it('marks every place the words occur, without case', () => {
    expect(markedPieces('Fix the path, then the PATH again', 'path')).toEqual([
      { text: 'Fix the ', marked: false },
      { text: 'path', marked: true },
      { text: ', then the ', marked: false },
      { text: 'PATH', marked: true },
      { text: ' again', marked: false },
    ])
  })

  it('leaves text whole when nothing matches or nothing is asked', () => {
    expect(markedPieces('add a ledger', 'path')).toEqual([{ text: 'add a ledger', marked: false }])
    expect(markedPieces('add a ledger', '  ')).toEqual([{ text: 'add a ledger', marked: false }])
  })

  it('marks a match at the very start and the very end', () => {
    expect(markedPieces('abcab', 'ab').map((piece) => piece.marked)).toEqual([true, false, true])
  })
})
