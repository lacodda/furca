import { useSyncExternalStore } from 'react'
import { Channel } from '@tauri-apps/api/core'
import { api, failure } from './api'
import type { Fetched } from './generated/Fetched'
import type { Progress } from './generated/Progress'
import type { Pulled } from './generated/Pulled'
import type { Pushes } from './generated/Pushes'
import type { Reaching } from './generated/Reaching'
import type { Reconcile } from './generated/Reconcile'
import type { Remotes } from './generated/Remotes'
import type { Deleted } from './generated/Deleted'
import type { Switched } from './generated/Switched'
import type { TagDeletes } from './generated/TagDeletes'
import type { TagPushes } from './generated/TagPushes'

/*
 * The runs the window makes on a repository's remotes and refs - fetch, pull,
 * push, prune; a checkout, a branch or a tag deleted, a tag pushed - one
 * repository at a time, kept per repository while the window is: the toolbar
 * draws which one is going and how far it has got, the foot of the window says
 * it in words, and what came of the last one stays until the next, or, when
 * it went well, for a few seconds.
 *
 * furca-desktop runs one of each kind at a time (src-tauri/src/remote.rs,
 * src-tauri/src/refs.rs); the window holds the buttons back while one goes, so
 * a second click is not queued behind the first without a word. What is made
 * in a dialog - a branch, a tag, a rename - says how it went in the dialog.
 */

export type Doing =
  | 'fetch'
  | 'pull'
  | 'push'
  | 'prune'
  | 'remove'
  | 'checkout'
  | 'delete-branch'
  | 'unfollow'
  | 'push-tag'
  | 'delete-tag'

/** A run going now. */
export interface Run {
  doing: Doing
  /** The remote being reached, when they are reached one at a time. */
  remote: string | null
  /** How far its current phase has got, once git has said. */
  progress: Progress | null
}

/** What came of the last run. */
export type Outcome =
  | { ok: true; doing: 'fetch' | 'prune'; fetched: Fetched }
  | { ok: true; doing: 'pull'; pulled: Pulled }
  | { ok: true; doing: 'push'; pushes: Pushes }
  | { ok: true; doing: 'remove'; remote: string }
  | { ok: true; doing: 'checkout'; switched: Switched }
  | { ok: true; doing: 'delete-branch'; deleted: Deleted }
  | { ok: true; doing: 'unfollow'; branch: string }
  | { ok: true; doing: 'push-tag'; tag: string; pushes: TagPushes }
  | { ok: true; doing: 'delete-tag'; tag: string; deletes: TagDeletes }
  /** `reached` are the remotes that took it, when some did and some did not. */
  | { ok: false; doing: Doing; reason: string; reached?: string[] }

export interface Reach {
  run: Run | null
  outcome: Outcome | null
}

/** How long what went well stays on the screen. */
const SHOWN_FOR = 6000

const NOTHING: Reach = { run: null, outcome: null }
const entries = new Map<string, Reach>()
const timers = new Map<string, ReturnType<typeof setTimeout>>()
const listeners = new Set<() => void>()

function set(path: string, reach: Reach) {
  entries.set(path, reach)
  for (const listener of listeners) listener()
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** The run going on the repository at `path`, and what came of the last one. */
export function useReach(path: string): Reach {
  return useSyncExternalStore(subscribe, () => entries.get(path) ?? NOTHING)
}

/** Whether a run is going on the repository at `path` now. */
export function reaching(path: string): boolean {
  return (entries.get(path) ?? NOTHING).run !== null
}

/** Takes away what came of the last run. */
export function dismiss(path: string) {
  clearTimeout(timers.get(path))
  set(path, { run: entries.get(path)?.run ?? null, outcome: null })
}

/** Runs `start` as `doing`, hearing how far it gets, and keeps what came of
 * it. Resolves with the outcome, whichever way it went. */
async function go(
  path: string,
  doing: Doing,
  start: (heard: Channel<Reaching>) => Promise<Outcome>,
  remote: string | null = null,
): Promise<Outcome> {
  clearTimeout(timers.get(path))
  set(path, { run: { doing, remote, progress: null }, outcome: null })
  const heard = new Channel<Reaching>()
  heard.onmessage = ({ remote: from, progress }) => {
    const now = entries.get(path)
    if (now?.run?.doing !== doing) return
    set(path, { ...now, run: { doing, remote: from ?? now.run.remote, progress } })
  }
  const outcome = await start(heard).catch((cause: unknown): Outcome => ({ ok: false, doing, reason: failure(cause) }))
  set(path, { run: null, outcome })
  if (outcome.ok) {
    timers.set(
      path,
      setTimeout(() => {
        if (entries.get(path)?.outcome === outcome) set(path, { run: null, outcome: null })
      }, SHOWN_FOR),
    )
  }
  return outcome
}

export function fetchFrom(path: string, from: Remotes, prune = false): Promise<Outcome> {
  const remote = from.to === 'named' ? from.name : null
  return go(path, 'fetch', (heard) => api.fetch(path, from, prune, heard).then((fetched): Outcome => ({ ok: true, doing: 'fetch', fetched })), remote)
}

export function pullInto(path: string, how: Reconcile): Promise<Outcome> {
  return go(path, 'pull', (heard) => api.pull(path, how, heard).then((pulled): Outcome => ({ ok: true, doing: 'pull', pulled })))
}

export function pushTo(path: string, to: Remotes, force = false): Promise<Outcome> {
  const remote = to.to === 'named' ? to.name : null
  return go(
    path,
    'push',
    (heard) =>
      api.push(path, to, force, heard).then((pushes): Outcome => {
        if (pushes.failed.length === 0) return { ok: true, doing: 'push', pushes }
        return { ok: false, doing: 'push', reason: refusals(pushes), reached: pushes.pushed.map((each) => each.remote) }
      }),
    remote,
  )
}

export function pruneRemote(path: string, name: string): Promise<Outcome> {
  return go(path, 'prune', () => api.remotePrune(path, name).then((fetched): Outcome => ({ ok: true, doing: 'prune', fetched })), name)
}

/** Removes the remote `name`; the window has asked first. */
export function removeRemote(path: string, name: string): Promise<Outcome> {
  return go(path, 'remove', () => api.remoteRemove(path, name).then((): Outcome => ({ ok: true, doing: 'remove', remote: name })), name)
}

/** Checks out the branch `name`: a local one, or a remote-tracking one as the
 * local branch that follows it. */
export function checkoutBranch(path: string, name: string): Promise<Outcome> {
  return go(path, 'checkout', () => api.checkout(path, name).then((switched): Outcome => ({ ok: true, doing: 'checkout', switched })))
}

/** Checks out the commit `revision` names, on no branch. */
export function checkoutCommit(path: string, revision: string): Promise<Outcome> {
  return go(path, 'checkout', () =>
    api.checkoutDetached(path, revision).then((switched): Outcome => ({ ok: true, doing: 'checkout', switched })),
  )
}

/** Deletes the branch `name`, which the window said holds `lost` commits
 * nothing else has. */
export function deleteBranch(path: string, name: string, lost: number): Promise<Outcome> {
  return go(path, 'delete-branch', () =>
    api.branchDelete(path, name, lost).then((deleted): Outcome => ({ ok: true, doing: 'delete-branch', deleted })),
  )
}

/** Has the branch `name` follow nothing. */
export function unfollow(path: string, name: string): Promise<Outcome> {
  return go(path, 'unfollow', () => api.branchFollow(path, name, null).then((): Outcome => ({ ok: true, doing: 'unfollow', branch: name })))
}

/** Pushes the tag `name` to the remotes `to` names. */
export function pushTag(path: string, name: string, to: Remotes): Promise<Outcome> {
  return go(
    path,
    'push-tag',
    (heard) =>
      api.tagPush(path, name, to, heard).then((pushes): Outcome => {
        if (pushes.failed.length === 0) return { ok: true, doing: 'push-tag', tag: name, pushes }
        return { ok: false, doing: 'push-tag', reason: refusals(pushes), reached: pushes.pushed.map((each) => each.remote) }
      }),
    to.to === 'named' ? to.name : null,
  )
}

/** Deletes the tag `name` here, and takes it away from `remotes` first; the
 * window has asked. */
export function deleteTag(path: string, name: string, remotes: string[]): Promise<Outcome> {
  return go(
    path,
    'delete-tag',
    (heard) =>
      api.tagDelete(path, name, remotes, heard).then((deletes): Outcome => {
        if (deletes.failed.length === 0) return { ok: true, doing: 'delete-tag', tag: name, deletes }
        return { ok: false, doing: 'delete-tag', reason: refusals(deletes), reached: deletes.remotes.map((each) => each.remote) }
      }),
    remotes.length === 1 ? (remotes[0] ?? null) : null,
  )
}

/** What each remote that refused a push said, a remote at a time; git's own
 * words, as the console has them. */
export function refusals(pushes: { failed: { remote: string; error: string }[] }): string {
  return pushes.failed.map((failed) => `${failed.remote}: ${failed.error}`).join('\n\n')
}

/** A phase of progress in a few words: `Receiving objects 45%`, or the count
 * while git is still counting. */
export function progressWords(progress: Progress): string {
  const known = progress.total !== null && progress.total > 0
  const count = known
    ? `${String(Math.floor((Math.min(progress.done, progress.total ?? 0) * 100) / (progress.total ?? 1)))}%`
    : String(progress.done)
  return `${progress.phase} ${count}`
}

/** How a summary of an outcome that went well is said: the key of its words
 * and what goes in them. */
export interface Said {
  key: string
  values: Record<string, string | number>
}

export function summary(outcome: Outcome & { ok: true }): Said {
  switch (outcome.doing) {
    case 'fetch': {
      const remotes = outcome.fetched.remotes.join(', ')
      const count = outcome.fetched.moved.length
      return count === 0 ? { key: 'reach.fetchedNothing', values: { remotes } } : { key: 'reach.fetched', values: { remotes, count } }
    }
    case 'prune': {
      const remote = outcome.fetched.remotes.join(', ')
      const count = outcome.fetched.moved.length
      return count === 0 ? { key: 'reach.prunedNothing', values: { remote } } : { key: 'reach.pruned', values: { remote, count } }
    }
    case 'pull': {
      const { branch, upstream, from, to } = outcome.pulled
      return from === to ? { key: 'reach.upToDate', values: { branch, upstream } } : { key: 'reach.pulled', values: { branch, upstream } }
    }
    case 'remove':
      return { key: 'reach.removed', values: { remote: outcome.remote } }
    case 'checkout': {
      const { branch, commit, created, upstream } = outcome.switched
      if (branch === null) return { key: 'refs.checkedOutDetached', values: { at: commit === null ? '' : commit.slice(0, 7) } }
      if (created && upstream !== null) return { key: 'refs.checkedOutFollowing', values: { branch, upstream } }
      return { key: 'refs.checkedOut', values: { branch } }
    }
    case 'delete-branch': {
      const { name, target, lost } = outcome.deleted
      return { key: 'refs.deletedBranch', values: { name, at: target.slice(0, 7), count: lost } }
    }
    case 'unfollow':
      return { key: 'refs.unfollowed', values: { branch: outcome.branch } }
    case 'push-tag':
      return { key: 'refs.pushedTag', values: { tag: outcome.tag, remotes: outcome.pushes.pushed.map((each) => each.remote).join(', ') } }
    case 'delete-tag': {
      const remotes = outcome.deletes.remotes.map((each) => each.remote).join(', ')
      return remotes === ''
        ? { key: 'refs.deletedTag', values: { tag: outcome.tag } }
        : { key: 'refs.deletedTagThere', values: { tag: outcome.tag, remotes } }
    }
    case 'push': {
      const pushed = outcome.pushes.pushed
      const branch = pushed[0]?.branch ?? ''
      const remotes = pushed.map((each) => each.remote).join(', ')
      const followed = pushed.find((each) => each.upstream_set)
      return followed
        ? { key: 'reach.pushedFollowing', values: { branch, remotes, remote: followed.remote } }
        : { key: pushed.some((each) => each.forced) ? 'reach.pushedForced' : 'reach.pushed', values: { branch, remotes } }
    }
  }
}
