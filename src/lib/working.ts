import { useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import type { Conflicted } from './generated/Conflicted'
import type { Counts } from './generated/Counts'
import type { FileChange } from './generated/FileChange'
import type { Status } from './generated/Status'
import type { WorkingCopy } from './generated/WorkingCopy'

/*
 * The working copy of each repository as the window last heard it. It is
 * pushed, not asked for: furca-desktop keeps the status of the tab in front and
 * says when its list moved (`src-tauri/src/working.rs`). What was heard of a
 * tab stays when the tab goes behind another, so coming back shows it at once
 * while it is read again.
 */

/** One repository's working copy, as the window sees it now. */
export type Held =
  | { state: 'reading' }
  | { state: 'failed'; reason: string }
  | {
      state: 'read'
      status: Status
      counts: Counts
      /** Why nothing is watching the working tree, if nothing is. */
      unwatched: string | null
      /** Goes up each time the whole list was read again. */
      everything: number
      /** Goes up, per path, each time that file was read again. */
      files: ReadonlyMap<string, number>
    }

const READING: Held = { state: 'reading' }

/** What a repository's working copy becomes on hearing `heard`. */
export function heard(held: Held, read: WorkingCopy['read']): Held {
  if (read.state === 'failed') return { state: 'failed', reason: read.reason }
  const everything = held.state === 'read' ? held.everything : 0
  const files = held.state === 'read' ? held.files : new Map<string, number>()
  const { refreshed } = read
  const next = {
    state: 'read',
    status: read.status,
    counts: read.counts,
    unwatched: read.unwatched,
    everything: refreshed.kind === 'everything' ? everything + 1 : everything,
    files,
  } as const
  if (refreshed.kind !== 'files') return next
  const bumped = new Map(files)
  for (const path of refreshed.paths) bumped.set(path, (bumped.get(path) ?? 0) + 1)
  return { ...next, files: bumped }
}

/** Which list of the working copy a file is in. */
export type Area = 'conflicted' | 'staged' | 'unstaged' | 'untracked'

/** A row of the working copy. */
export interface Chosen {
  area: Area
  path: string
}

/** The lists in the order the window draws them: what stops a commit, what
 * the commit would hold, what it would not. */
const AREAS = ['conflicted', 'staged', 'unstaged', 'untracked'] as const

function rowsOf(status: Status, area: Area): readonly (FileChange | Conflicted)[] {
  return status[area]
}

/**
 * The row to show, given the one that was chosen: it stays while it is there.
 * Once it has gone - staged, reverted, deleted - the row now in its place in
 * the same list takes over, so working down a list keeps one's place; then
 * the same file in another list, which is where staging moved it to; then the
 * first row of all. `null` when the working copy is clean.
 */
export function rowToShow(status: Status, chosen: Chosen | null, before: Status | null = null): Chosen | null {
  if (chosen !== null) {
    const rows = rowsOf(status, chosen.area)
    if (rows.some((row) => row.path === chosen.path)) return chosen
    const elsewhere = AREAS.find((area) => rowsOf(status, area).some((row) => row.path === chosen.path))
    if (elsewhere !== undefined) return { area: elsewhere, path: chosen.path }
    if (rows.length > 0) {
      const at = before === null ? 0 : rowsOf(before, chosen.area).findIndex((row) => row.path === chosen.path)
      const place = rows[Math.min(Math.max(at, 0), rows.length - 1)]
      if (place !== undefined) return { area: chosen.area, path: place.path }
    }
  }
  for (const area of AREAS) {
    const first = rowsOf(status, area)[0]
    if (first !== undefined) return { area, path: first.path }
  }
  return null
}

/** What is not committed, in the words the bar above the graph counts it: a
 * file both staged and changed again is one changed file. */
export function counted(status: Status, counts: Counts): { changed: number; untracked: number; total: number } {
  const paths = new Set<string>()
  for (const file of status.staged) paths.add(file.path)
  for (const file of status.unstaged) paths.add(file.path)
  for (const file of status.conflicted) paths.add(file.path)
  // Past the rows the window is given the lists are cut, and a file in both
  // cannot be told from one in either: the longer one is the honest floor.
  const cut = counts.staged > status.staged.length || counts.unstaged > status.unstaged.length
  const changed = cut ? Math.max(counts.staged, counts.unstaged, paths.size) : paths.size
  return { changed, untracked: counts.untracked, total: changed + counts.untracked }
}

const held = new Map<string, Held>()
const listeners = new Set<() => void>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

let listening: Promise<unknown> | null = null

/** Starts listening, for as long as the window is there; resolves once it is
 * listening, which is when something can be watched without its first answer
 * going unheard. */
export function listenForWorkingCopy(): Promise<unknown> {
  listening ??= listen<WorkingCopy>('working-copy', ({ payload }) => {
    held.set(payload.path, heard(held.get(payload.path) ?? READING, payload.read))
    for (const listener of listeners) listener()
  })
  return listening
}

/** Forgets what was heard of the repository at `path`: its tab closed. */
export function forgetWorkingCopy(path: string): void {
  held.delete(path)
  for (const listener of listeners) listener()
}

/** The working copy of the repository at `path`. */
export function useWorkingCopy(path: string): Held {
  return useSyncExternalStore(subscribe, () => held.get(path) ?? READING)
}

/** What a file's change is read under, so one that changed is read again. */
export function readAs(copy: Held, chosen: Chosen): string {
  if (copy.state !== 'read') return ''
  return `working\n${chosen.area}\n${String(copy.everything)}.${String(copy.files.get(chosen.path) ?? 0)}`
}
