import { describe, expect, it } from 'vitest'
import type { FileChange } from './generated/FileChange'
import type { Status } from './generated/Status'
import type { WorkingCopy } from './generated/WorkingCopy'
import { counted, heard, readAs, rowToShow, type Held } from './working'

const file = (path: string): FileChange => ({
  path,
  status: 'modified',
  from: null,
  similarity: null,
  before: null,
  after: null,
  lines: null,
})

const status = (lists: Partial<Record<'staged' | 'unstaged' | 'untracked', string[]>>, conflicted: string[] = []): Status => ({
  staged: (lists.staged ?? []).map(file),
  unstaged: (lists.unstaged ?? []).map(file),
  untracked: (lists.untracked ?? []).map(file),
  conflicted: conflicted.map((path) => ({ path, kind: 'both-modified' })),
})

const read = (refreshed: Extract<WorkingCopy['read'], { state: 'read' }>['refreshed'], of: Status = status({})): WorkingCopy['read'] => ({
  state: 'read',
  status: of,
  counts: { staged: of.staged.length, unstaged: of.unstaged.length, untracked: of.untracked.length },
  refreshed,
  unwatched: null,
})

describe('hearing the working copy', () => {
  const reading: Held = { state: 'reading' }

  it('takes the first list it hears', () => {
    const next = heard(reading, read({ kind: 'everything' }, status({ unstaged: ['a'] })))
    expect(next.state).toBe('read')
    if (next.state === 'read') expect(next.status.unstaged[0]?.path).toBe('a')
  })

  it('says a file is stale only when that file was read again', () => {
    const first = heard(reading, read({ kind: 'everything' }))
    const chosen = { area: 'unstaged', path: 'a' } as const
    const other = { area: 'unstaged', path: 'b' } as const
    const asFirst = readAs(first, chosen)
    const otherFirst = readAs(first, other)

    const saved = heard(first, read({ kind: 'files', paths: ['a'] }))
    expect(readAs(saved, chosen)).not.toBe(asFirst)
    expect(readAs(saved, other)).toBe(otherFirst)
  })

  it('takes every file as stale when the whole list was read again', () => {
    const first = heard(reading, read({ kind: 'everything' }))
    const again = heard(first, read({ kind: 'everything' }))
    const chosen = { area: 'staged', path: 'a' } as const
    expect(readAs(again, chosen)).not.toBe(readAs(first, chosen))
  })

  it('keeps a file as it was while the rest of a long list fills in', () => {
    const first = heard(reading, read({ kind: 'files', paths: ['a'] }))
    const filling = heard(first, read({ kind: 'nothing' }))
    const chosen = { area: 'unstaged', path: 'a' } as const
    expect(readAs(filling, chosen)).toBe(readAs(first, chosen))
  })

  it('tells a file apart by its list: the same path staged and changed again', () => {
    const copy = heard(reading, read({ kind: 'everything' }))
    expect(readAs(copy, { area: 'staged', path: 'a' })).not.toBe(readAs(copy, { area: 'unstaged', path: 'a' }))
  })

  it('takes a failure, and comes back from it', () => {
    const failed = heard(reading, { state: 'failed', reason: 'gone' })
    expect(failed).toEqual({ state: 'failed', reason: 'gone' })
    expect(heard(failed, read({ kind: 'everything' })).state).toBe('read')
  })
})

describe('the row to show', () => {
  it('opens on the first row, in the order the lists are drawn', () => {
    expect(rowToShow(status({ unstaged: ['b'], staged: ['a'] }), null)).toEqual({ area: 'staged', path: 'a' })
    expect(rowToShow(status({ unstaged: ['b'] }, ['c']), null)).toEqual({ area: 'conflicted', path: 'c' })
    expect(rowToShow(status({ untracked: ['n'] }), null)).toEqual({ area: 'untracked', path: 'n' })
  })

  it('is nothing on a clean tree', () => {
    expect(rowToShow(status({}), { area: 'staged', path: 'a' })).toBeNull()
  })

  it('stays where it is while the file is there', () => {
    const chosen = { area: 'unstaged', path: 'b' } as const
    expect(rowToShow(status({ unstaged: ['a', 'b'], staged: ['c'] }), chosen)).toBe(chosen)
  })

  it('follows a file that moved to another list', () => {
    const chosen = { area: 'unstaged', path: 'b' } as const
    expect(rowToShow(status({ staged: ['b'], unstaged: ['a'] }), chosen)).toEqual({ area: 'staged', path: 'b' })
  })

  it('keeps its place in the list when the file has gone from all of them', () => {
    const before = status({ unstaged: ['a', 'b', 'c'] })
    const chosen = { area: 'unstaged', path: 'b' } as const
    expect(rowToShow(status({ unstaged: ['a', 'c'] }), chosen, before)).toEqual({ area: 'unstaged', path: 'c' })
    const last = { area: 'unstaged', path: 'c' } as const
    expect(rowToShow(status({ unstaged: ['a', 'b'] }), last, before)).toEqual({ area: 'unstaged', path: 'b' })
  })

  it('goes to another list when its own has emptied', () => {
    const chosen = { area: 'unstaged', path: 'a' } as const
    expect(rowToShow(status({ untracked: ['n'] }), chosen)).toEqual({ area: 'untracked', path: 'n' })
  })
})

describe('counting what is not committed', () => {
  it('counts a file staged and changed again once', () => {
    const of = status({ staged: ['a', 'b'], unstaged: ['b', 'c'], untracked: ['n'] }, ['x'])
    const counts = { staged: 2, unstaged: 2, untracked: 1 }
    expect(counted(of, counts)).toEqual({ changed: 4, untracked: 1, total: 5 })
  })

  it('takes the true lengths when the lists were cut for the window', () => {
    const of = status({ staged: ['a'], unstaged: ['b'] })
    const counts = { staged: 5000, unstaged: 3000, untracked: 7000 }
    expect(counted(of, counts)).toEqual({ changed: 5000, untracked: 7000, total: 12000 })
  })
})
