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

/*
 * The runs on a remote the window makes - fetch, pull, push, prune - one
 * repository at a time, kept per repository while the window is: the toolbar
 * draws which one is going and how far it has got, the foot of the window says
 * it in words, and what came of the last one stays until the next, or, when
 * it went well, for a few seconds.
 *
 * furca-desktop runs one of them at a time (src-tauri/src/remote.rs); the
 * window holds the buttons back while one goes, so a second click is not
 * queued behind the first without a word.
 */

export type Doing = 'fetch' | 'pull' | 'push' | 'prune' | 'remove'

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
  | { ok: false; doing: Doing; reason: string; pushes?: Pushes }

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
        return { ok: false, doing: 'push', reason: refusals(pushes), pushes }
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

/** What each remote that refused a push said, a remote at a time; git's own
 * words, as the console has them. */
export function refusals(pushes: Pushes): string {
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
