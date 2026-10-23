import { useEffect, useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api, failure } from './api'
import { forgetGraph, graphComplete, openGraph } from './graph'
import type { Maintenance } from './generated/Maintenance'
import type { Sidebar } from './generated/Sidebar'
import type { Standing } from './generated/Standing'
import type { Step } from './generated/Step'

/*
 * What the window has read of each open repository, kept between tab
 * switches: going back to a tab shows what it showed, at once, rather than a
 * spinner for something read a moment ago.
 *
 * A read is asked for once per repository and shared by whoever draws it. A
 * failure is kept too - "this repository is gone" is an answer, and asking
 * again on every render would only repeat it.
 */

export type Read<T> = { state: 'reading' } | { state: 'read'; value: T } | { state: 'failed'; reason: string }

/** One object for every read not in yet: `useSyncExternalStore` compares
 * snapshots by identity, and a fresh `{ state: 'reading' }` per call would
 * read as a change on every render, forever. */
const READING = { state: 'reading' } as const

class Reads<T> {
  private readonly entries = new Map<string, Read<T>>()
  private readonly pending = new Map<string, Promise<void>>()
  private readonly listeners = new Set<() => void>()
  private readonly read: (path: string) => Promise<T>

  constructor(read: (path: string) => Promise<T>) {
    this.read = read
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  get(path: string): Read<T> {
    return this.entries.get(path) ?? READING
  }

  /** Reads `path` unless it has been read or is being read; resolves when
   * the read is over, whichever way it went. */
  load(path: string): Promise<void> {
    if (this.entries.has(path)) return Promise.resolve()
    return this.pending.get(path) ?? this.start(path)
  }

  /** Reads `path` again, keeping what was read on the screen until the new
   * answer arrives: coming back to a tab, or to the window, shows the old
   * column for the moment the read takes rather than a spinner. */
  refresh(path: string): Promise<void> {
    return this.pending.get(path) ?? this.start(path)
  }

  private start(path: string): Promise<void> {
    const reading = this.read(path).then(
      (value) => this.set(path, { state: 'read', value }),
      (cause: unknown) => this.set(path, { state: 'failed', reason: failure(cause) }),
    )
    this.pending.set(path, reading)
    return reading
  }

  /** Forgets what was read of `path`: its tab closed, and a tab opened on
   * it later must not show what it held then. */
  forget(path: string) {
    this.entries.delete(path)
    for (const listener of this.listeners) listener()
  }

  private set(path: string, entry: Read<T>) {
    this.pending.delete(path)
    this.entries.set(path, entry)
    for (const listener of this.listeners) listener()
  }
}

export const sidebars = new Reads<Sidebar>(api.sidebar)
export const standings = new Reads<Standing>(api.standing)

function useRead<T>(reads: Reads<T>, path: string): Read<T> {
  useEffect(() => {
    void reads.load(path)
  }, [reads, path])
  return useSyncExternalStore(reads.subscribe, () => reads.get(path))
}

/** What the left column shows of the repository at `path`. */
export const useSidebar = (path: string) => useRead(sidebars, path)

/** Where the repository at `path` stands. */
export const useStanding = (path: string) => useRead(standings, path)

/** Reads the repository at `path` again: its column, where it stands, and
 * its graph, which is begun again only if the refs moved. */
export function refresh(path: string): void {
  void sidebars.refresh(path)
  void standings.refresh(path)
  void openGraph(path)
}

/** Forgets what was read of the repository at `path`. */
export function forget(path: string): void {
  sidebars.forget(path)
  standings.forget(path)
  forgetGraph(path)
}

/** Reads every one of `paths` in turn, one after another rather than all at
 * once: the line is eighteen repositories, and the one in front was already
 * read first. A graph counts as read once it is laid out to its end, so
 * going to any tab shows its history at once. */
export async function readAll(paths: readonly string[]): Promise<void> {
  for (const path of paths) {
    await sidebars.load(path)
    await standings.load(path)
    await openGraph(path)
    await graphComplete(path)
  }
}

/* The background work furca-desktop does on a repository - git writing its
 * commit-graph - heard as events, the latest step kept per repository. */
const steps = new Map<string, Step>()
const stepListeners = new Set<() => void>()

/** Starts listening for background work; once, from the shell. */
export function listenForMaintenance(): () => void {
  const stop = listen<Maintenance>('maintenance', ({ payload }) => {
    steps.set(payload.path, payload.step)
    for (const listener of stepListeners) listener()
  })
  return () => {
    void stop.then((unlisten) => unlisten())
  }
}

const subscribeSteps = (listener: () => void) => {
  stepListeners.add(listener)
  return () => {
    stepListeners.delete(listener)
  }
}

/** The latest background step on the repository at `path`, if any. */
export function useMaintenance(path: string): Step | undefined {
  return useSyncExternalStore(subscribeSteps, () => steps.get(path))
}
