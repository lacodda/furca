import { describe, expect, it } from 'vitest'
import type { Progress } from './generated/Progress'
import type { Pushed } from './generated/Pushed'
import { progressWords, refusals, summary } from './remote'

const progress = (done: number, total: number | null): Progress => ({
  phase: 'Receiving objects',
  remote: false,
  done,
  total,
  finished: false,
})

const pushed = (remote: string, extra: Partial<Pushed> = {}): Pushed => ({
  remote,
  branch: 'main',
  to: 'refs/heads/main',
  commit: '4c3ea3f0',
  forced: false,
  upstream_set: false,
  ...extra,
})

describe('progressWords', () => {
  it('says a phase with its share once git knows the total', () => {
    expect(progressWords(progress(450, 1000))).toBe('Receiving objects 45%')
    expect(progressWords(progress(7, 5))).toBe('Receiving objects 100%')
  })

  it('says the count while git is still counting', () => {
    expect(progressWords(progress(87, null))).toBe('Receiving objects 87')
    expect(progressWords(progress(3, 0))).toBe('Receiving objects 3')
  })
})

describe('summary', () => {
  it('tells a fetch that brought nothing from one that moved refs', () => {
    const fetched = { remotes: ['mirror', 'origin'], moved: [] }
    expect(summary({ ok: true, doing: 'fetch', fetched })).toEqual({
      key: 'reach.fetchedNothing',
      values: { remotes: 'mirror, origin' },
    })
    const moved = { remotes: ['origin'], moved: [{ name: 'refs/remotes/origin/main', from: 'a', to: 'b' }] }
    expect(summary({ ok: true, doing: 'fetch', fetched: moved })).toEqual({
      key: 'reach.fetched',
      values: { remotes: 'origin', count: 1 },
    })
  })

  it('tells a pull that came to nothing from one that moved the branch', () => {
    const pulled = { branch: 'main', upstream: 'origin/main', from: 'a', to: 'a' }
    expect(summary({ ok: true, doing: 'pull', pulled }).key).toBe('reach.upToDate')
    expect(summary({ ok: true, doing: 'pull', pulled: { ...pulled, to: 'b' } }).key).toBe('reach.pulled')
  })

  it('names the remote a new branch follows after its first push', () => {
    const pushes = { pushed: [pushed('mirror'), pushed('origin', { upstream_set: true })], failed: [] }
    expect(summary({ ok: true, doing: 'push', pushes })).toEqual({
      key: 'reach.pushedFollowing',
      values: { branch: 'main', remotes: 'mirror, origin', remote: 'origin' },
    })
    const forced = { pushed: [pushed('origin', { forced: true })], failed: [] }
    expect(summary({ ok: true, doing: 'push', pushes: forced }).key).toBe('reach.pushedForced')
  })
})

describe('refusals', () => {
  it('gives each refusing remote its own words', () => {
    const pushes = {
      pushed: [pushed('origin')],
      failed: [
        { remote: 'mirror', error: '! [rejected] main -> main (fetch first)' },
        { remote: 'backup', error: 'Could not resolve host' },
      ],
    }
    expect(refusals(pushes)).toBe('mirror: ! [rejected] main -> main (fetch first)\n\nbackup: Could not resolve host')
  })
})

describe('summary of a checkout, a deletion and a tag', () => {
  it('tells a checkout of a branch from a new one that follows, and from no branch', () => {
    const switched = { branch: 'main', commit: '7012074b', created: false, upstream: 'origin/main' }
    expect(summary({ ok: true, doing: 'checkout', switched })).toEqual({ key: 'refs.checkedOut', values: { branch: 'main' } })
    expect(summary({ ok: true, doing: 'checkout', switched: { ...switched, branch: 'login', created: true, upstream: 'origin/login' } })).toEqual({
      key: 'refs.checkedOutFollowing',
      values: { branch: 'login', upstream: 'origin/login' },
    })
    expect(summary({ ok: true, doing: 'checkout', switched: { ...switched, branch: null, upstream: null } })).toEqual({
      key: 'refs.checkedOutDetached',
      values: { at: '7012074' },
    })
  })

  it('says where a deleted branch was, and how many commits went with it', () => {
    const deleted = { name: 'topic', target: '7f29a8e0', lost: 2 }
    expect(summary({ ok: true, doing: 'delete-branch', deleted })).toEqual({
      key: 'refs.deletedBranch',
      values: { name: 'topic', at: '7f29a8e', count: 2 },
    })
  })

  it('names the remotes a tag was taken away from, and none when it went from here only', () => {
    const deleted = { name: 'v1', target: 'a', lost: 0 }
    const here = { deleted, remotes: [], failed: [] }
    expect(summary({ ok: true, doing: 'delete-tag', tag: 'v1', deletes: here }).key).toBe('refs.deletedTag')
    const there = { ...here, remotes: [{ remote: 'origin', tag: 'v1', deleted: true }] }
    expect(summary({ ok: true, doing: 'delete-tag', tag: 'v1', deletes: there })).toEqual({
      key: 'refs.deletedTagThere',
      values: { tag: 'v1', remotes: 'origin' },
    })
  })
})
