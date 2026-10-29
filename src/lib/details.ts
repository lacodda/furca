import { useEffect, useRef, useState } from 'react'
import { api, failure } from './api'
import type { Details } from './generated/Details'
import type { FileChange } from './generated/FileChange'
import type { FileDiff } from './generated/FileDiff'
import type { Read } from './repos'

/*
 * A commit's details and one file's change, read when the reader chooses
 * them.
 *
 * A commit never changes, so what was read of one is kept by its id - going
 * back to a commit shows it at once - up to a few dozen commits and files.
 * Only the latest choice is answered: arrows held in the graph choose a row
 * every few milliseconds, and an answer for a row already left is dropped.
 * Until the new answer comes, the one before stays on the screen, marked as
 * the one before, rather than a spinner flashing between two commits.
 */

const KEPT = 64

/** The newest reads by key; the oldest goes when there are too many. */
class Kept<T> {
  private readonly values = new Map<string, T>()

  get(key: string): T | undefined {
    return this.values.get(key)
  }

  set(key: string, value: T) {
    this.values.delete(key)
    this.values.set(key, value)
    if (this.values.size > KEPT) {
      const oldest = this.values.keys().next().value
      if (oldest !== undefined) this.values.delete(oldest)
    }
  }
}

const details = new Kept<Details>()
const diffs = new Kept<FileDiff>()

const READING = { state: 'reading' } as const

/** What a choice shows: its read, or - while that is on its way - the read
 * of the choice before, with `stale` set. */
export interface Shown<T> {
  read: Read<T>
  stale: boolean
}

function useLatest<T>(key: string | null, kept: Kept<T>, read: () => Promise<T>): Shown<T> | null {
  const [last, setLast] = useState<{ key: string; read: Read<T> } | null>(null)
  // The key says what to read; the function is only how, and a new one on
  // every render is not a reason to read again.
  const reader = useRef(read)
  useEffect(() => {
    reader.current = read
  })

  useEffect(() => {
    if (key === null || kept.get(key) !== undefined) return
    let current = true
    reader.current().then(
      (value) => {
        kept.set(key, value)
        if (current) setLast({ key, read: { state: 'read', value } })
      },
      (cause: unknown) => {
        if (current) setLast({ key, read: { state: 'failed', reason: failure(cause) } })
      },
    )
    return () => {
      current = false
    }
  }, [key, kept])

  if (key === null) return null
  const value = kept.get(key)
  if (value !== undefined) return { read: { state: 'read', value }, stale: false }
  if (last?.key === key) return { read: last.read, stale: false }
  return last === null ? { read: READING, stale: false } : { read: last.read, stale: true }
}

/** The commit `id` of the repository at `path`, as a reader opens it. */
export function useDetails(path: string, id: string | null): Shown<Details> | null {
  return useLatest(id === null ? null : `${path}\n${id}`, details, () => api.details(path, id ?? ''))
}

/** One file's change in the commit `commit`. */
export function useFileDiff(path: string, commit: string, file: FileChange | null): Shown<FileDiff> | null {
  return useLatest(file === null ? null : `${path}\n${commit}\n${file.path}`, diffs, () => {
    if (file === null) return Promise.reject(new Error('no file'))
    markAsked()
    return api.fileDiff(path, file)
  })
}

/* The first diff the window opens is the one a measurement times, from the
 * moment it is asked for to the frame that shows it. */
let first: 'none' | 'asked' | 'shown' = 'none'

function markAsked() {
  if (first !== 'none') return
  first = 'asked'
  void api.measured('diff-asked')
}

/** Tells a measurement a diff is on the screen: the first one only. */
export function markShown() {
  if (first !== 'asked') return
  first = 'shown'
  void api.measured('diff-shown')
}
