import { describe, expect, it } from 'vitest'
import type { RebasePlan } from './generated/RebasePlan'
import { changed, combined, leaves, moved, rowsOf, todoOf, whyNot } from './rebase'

const person = { name: 'Ada', email: 'ada@example.com', time: '2026-01-01T00:00:00+00:00', seconds: 0 }

function plan(...subjects: string[]): RebasePlan {
  return {
    branch: 'main',
    onto: null,
    steps: subjects.map((subject, at) => ({
      commit: { id: `id${String(at)}`, parents: [], author: person, committer: person, subject },
      message: `${subject}\n\nBody of ${subject}.`,
      pushed: at === 0,
    })),
  }
}

describe('the rows of a rebase', () => {
  const rows = rowsOf(plan('feat: a', 'feat: b', 'feat: c'))
  const original = rows.map((row) => row.id)

  it('start as the plan has them, each picked', () => {
    expect(rows.map((row) => row.act)).toEqual(['pick', 'pick', 'pick'])
    expect(rows[0]?.pushed).toBe(true)
    expect(whyNot(rows, original)).toEqual({ why: 'unchanged' })
  })

  it('move to another place, and stay put when the place is not there', () => {
    expect(moved(rows, 2, 0).map((row) => row.id)).toEqual(['id2', 'id0', 'id1'])
    expect(moved(rows, 0, 2).map((row) => row.id)).toEqual(['id1', 'id2', 'id0'])
    expect(moved(rows, 0, 3).map((row) => row.id)).toEqual(original)
    expect(whyNot(moved(rows, 2, 0), original)).toBeNull()
  })

  it('cannot fold the first commit into nothing', () => {
    const folded = changed(rows, 'id0', { act: 'fixup' })
    expect(whyNot(folded, original)).toEqual({ why: 'nothing-before', id: 'id0' })
    // A dropped commit is not one to fold into.
    const dropped = changed(changed(rows, 'id0', { act: 'drop' }), 'id1', { act: 'squash' })
    expect(whyNot(dropped, original)).toEqual({ why: 'nothing-before', id: 'id1' })
    expect(whyNot(changed(rows, 'id1', { act: 'squash' }), original)).toBeNull()
  })

  it('need a message for a commit that is reworded', () => {
    const empty = changed(rows, 'id1', { act: 'reword', message: '  ' })
    expect(whyNot(empty, original)).toEqual({ why: 'no-message', id: 'id1' })
  })

  it('become a list that names every commit once', () => {
    const edited = changed(changed(changed(moved(rows, 2, 0), 'id2', { act: 'reword', message: 'feat: c, first\n' }), 'id1', { act: 'fixup' }), 'id0', {
      act: 'reword',
    })
    expect(todoOf(edited)).toEqual([
      { commit: 'id2', action: 'reword', message: 'feat: c, first' },
      // Reworded to what it said already: a pick.
      { commit: 'id0', action: 'pick' },
      { commit: 'id1', action: 'fixup' },
    ])
    expect(leaves(edited)).toBe(2)
  })

  it('count a reword that says the same as nothing changed', () => {
    expect(whyNot(changed(rows, 'id1', { act: 'reword' }), original)).toEqual({ why: 'unchanged' })
  })
})

describe('a squash', () => {
  it('starts from the messages one after another, oldest first', () => {
    expect(combined(plan('feat: a', 'feat: b').steps)).toBe('feat: a\n\nBody of feat: a.\n\nfeat: b\n\nBody of feat: b.')
  })
})
