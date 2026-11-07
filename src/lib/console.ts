import { useEffect, useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api } from './api'
import type { ConsoleLine } from './generated/ConsoleLine'
import type { GitRun } from './generated/GitRun'

/*
 * What the console shows: every run of git this window made in a repository,
 * the release engine's and the background's with its own. The app keeps them
 * (`src-tauri/src/commit.rs`) and tells of each as it ends; the page holds what
 * it has heard, so opening the panel on a tab shows it at once.
 */

/** As many as the app keeps. */
const KEPT = 500

const NONE: readonly GitRun[] = []
const runs = new Map<string, readonly GitRun[]>()
const asked = new Set<string>()
const listeners = new Set<() => void>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function tell() {
  for (const listener of listeners) listener()
}

/** What a repository's console holds after hearing of `run`. */
export function withRun(held: readonly GitRun[], run: GitRun): readonly GitRun[] {
  const next = [...held, run]
  return next.length > KEPT ? next.slice(next.length - KEPT) : next
}

let listening: Promise<unknown> | null = null

/** Starts listening, for as long as the window is there. */
export function listenForConsole(): Promise<unknown> {
  listening ??= listen<ConsoleLine>('console', ({ payload }) => {
    runs.set(payload.path, withRun(runs.get(payload.path) ?? NONE, payload.run))
    tell()
  })
  return listening
}

/** Forgets what was heard of the repository at `path`: its tab closed. */
export function forgetConsole(path: string): void {
  runs.delete(path)
  asked.delete(path)
  tell()
}

/** The runs of git of the repository at `path`, oldest first. What happened
 * before anything listened is asked for once. */
export function useConsole(path: string): readonly GitRun[] {
  useEffect(() => {
    if (asked.has(path)) return
    asked.add(path)
    void api.consoleLog(path).then((earlier) => {
      if (earlier.length > 0) {
        // Whatever was heard meanwhile is in the app's list as well.
        runs.set(path, earlier)
        tell()
      }
    })
  }, [path])
  return useSyncExternalStore(subscribe, () => runs.get(path) ?? NONE)
}

/** Whether a run went wrong. */
export function failed(run: GitRun): boolean {
  return run.code !== 0
}

/** A run's command as a person would type it, each argument quoted when it
 * would not survive as one word: the same as `GitRun::command` in the core. */
export function commandLine(run: GitRun): string {
  return ['git', ...run.args.map((arg) => (arg === '' || /[ "']/.test(arg) ? `"${arg.replaceAll('"', '\\"')}"` : arg))].join(' ')
}

/** When a run started, as a clock reads it in `locale`. */
export function clock(at: number, locale: string): string {
  return new Date(at).toLocaleTimeString(locale, { hour12: false })
}
