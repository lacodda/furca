import { useEffect, useSyncExternalStore } from 'react'
import { Channel } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { api, failure } from './api'
import type { Finder } from './generated/Finder'
import type { Found } from './generated/Found'
import type { GraphRow } from './generated/GraphRow'
import type { GraphState } from './generated/GraphState'
import type { Grown } from './generated/Grown'

/*
 * The graphs of the open repositories, as the window holds them: how far
 * each is laid out, and the rows it has asked for lately - never the whole
 * history. The graph itself lives in furca-desktop (ADR 0012); a row is
 * asked for in pages when it comes into view, and pages a reader has
 * scrolled away from are let go once there are more than a few screens'
 * worth. A tab of a hundred thousand commits holds what a tab of five
 * hundred does.
 */

/** Rows asked for at once: the size furca-desktop sends a new graph's
 * first page in, so that page is page 0 here. */
export const PAGE = 100
/** Pages kept per repository: forty screens or so. */
const KEPT = 40

/** What a search has found so far. */
export interface Search {
  query: string
  /** The rows found, in order, by anyone. */
  rows: number[]
  /** The rows only nooma found: the words are in the body of the message. */
  inMessage: ReadonlySet<number>
  /** Who is still searching. */
  searching: ReadonlySet<Finder>
  /** Who could not search, and why. */
  failed: string | null
}

/** One repository's graph, as the window sees it now. */
export interface GraphView {
  state: GraphState | null
  failed: string | null
  search: Search | null
  /** Bumped whenever a page arrives, so a view redraws. */
  version: number
}

const NOTHING: GraphView = { state: null, failed: null, search: null, version: 0 }

interface Held {
  view: GraphView
  branch: string | null
  pages: Map<number, GraphRow[]>
  asked: Set<number>
  /** Resolved each time the graph is complete. */
  waiting: (() => void)[]
  /** The search running, by its own count, so an older one's answers are
   * dropped. */
  searchTicket: number
}

const held = new Map<string, Held>()
const listeners = new Set<() => void>()
/** The latest growth heard of each repository's graph, whichever graph it
 * was: furca-desktop starts laying out the rest before its answer to `graph`
 * reaches the window, so the first chunks can be heard before the window
 * knows the graph they belong to. */
const grown = new Map<string, GraphState>()

function changed() {
  for (const listener of listeners) listener()
}

function entry(path: string): Held {
  let found = held.get(path)
  if (!found) {
    found = { view: NOTHING, branch: null, pages: new Map(), asked: new Set(), waiting: [], searchTicket: 0 }
    held.set(path, found)
  }
  return found
}

function update(path: string, next: Partial<GraphView>) {
  const at = entry(path)
  at.view = { ...at.view, ...next }
  if (at.view.state?.complete) {
    for (const resolve of at.waiting.splice(0)) resolve()
  }
  changed()
}

/** Takes `state` as the graph of `path`. A new generation is a new graph:
 * what was asked of the old one goes. */
function take(path: string, answered: GraphState) {
  const heard = grown.get(path)
  const state = heard?.generation === answered.generation && heard.rows > answered.rows ? heard : answered
  const at = entry(path)
  if (at.view.state?.generation !== state.generation) {
    at.pages.clear()
    at.asked.clear()
    at.searchTicket += 1
    update(path, { state, failed: null, search: null, version: at.view.version + 1 })
  } else {
    update(path, { state, failed: null })
  }
}

/** Listening for graphs growing, begun with the first graph opened. */
let listening: Promise<unknown> | null = null

/** Opens the graph of `path` - or checks the one held is still current -
 * and reads its first page. Resolves once the first page is in. */
export async function openGraph(path: string, branch: string | null = entry(path).branch): Promise<void> {
  const at = entry(path)
  at.branch = branch
  try {
    listening ??= listen<Grown>('graph', ({ payload }) => {
      grown.set(payload.path, payload.state)
      const known = held.get(payload.path)
      // A graph the window no longer holds, or an older one of this tab.
      if (known?.view.state?.generation !== payload.state.generation) return
      update(payload.path, { state: payload.state })
    })
    await listening
    const answer = await api.graph(path, branch)
    take(path, answer.state)
    if (answer.first.length > 0) {
      at.pages.set(0, answer.first)
      update(path, { version: at.view.version + 1 })
    }
    await loadPage(path, 0)
  } catch (cause) {
    update(path, { failed: failure(cause) })
  }
}

/** Shows the history of `branch` (a full ref name), or of everything. */
export function showBranch(path: string, branch: string | null): Promise<void> {
  return openGraph(path, branch)
}

/** Resolves once the graph of `path` is laid out to its end. */
export function graphComplete(path: string): Promise<void> {
  const at = entry(path)
  if (at.view.state?.complete || at.view.failed !== null) return Promise.resolve()
  return new Promise((resolve) => at.waiting.push(resolve))
}

/** Lets go of the graph of `path`: its tab closed. */
export function forgetGraph(path: string): void {
  const at = held.get(path)
  held.delete(path)
  grown.delete(path)
  for (const resolve of at?.waiting ?? []) resolve()
  void api.forgetGraph(path)
  changed()
}

/** Whether the page held for `page` lacks rows the graph now has: a page
 * read while the graph was still being laid out comes back short. */
function short(at: Held, page: number): boolean {
  const kept = at.pages.get(page)
  const rows = at.view.state?.rows ?? 0
  return kept === undefined || page * PAGE + kept.length < Math.min(rows, (page + 1) * PAGE)
}

async function loadPage(path: string, page: number): Promise<void> {
  const at = entry(path)
  const state = at.view.state
  if (!state || !short(at, page) || at.asked.has(page)) return
  at.asked.add(page)
  try {
    const rows = await api.graphRows(path, state.generation, page * PAGE, PAGE)
    if (held.get(path) !== at || at.view.state?.generation !== state.generation) return
    at.pages.delete(page)
    at.pages.set(page, rows)
    while (at.pages.size > KEPT) {
      const oldest = at.pages.keys().next().value
      if (oldest === undefined) break
      at.pages.delete(oldest)
    }
    update(path, { version: at.view.version + 1 })
  } catch (cause) {
    // The graph was begun again under the window: take the new one.
    void openGraph(path)
    console.warn('furca: a page of the graph did not load:', failure(cause))
  } finally {
    at.asked.delete(page)
  }
}

/** Makes sure rows `start` to `end` are being read, and keeps their pages
 * from being let go. */
export function wantRows(path: string, start: number, end: number): void {
  const at = held.get(path)
  const rows = at?.view.state?.rows ?? 0
  if (!at || rows === 0) return
  const last = Math.min(end, rows) - 1
  for (let page = Math.floor(start / PAGE); page <= Math.floor(last / PAGE); page++) {
    const kept = at.pages.get(page)
    if (kept && !short(at, page)) {
      // Taken out and put back: the map's order is the order of last use.
      at.pages.delete(page)
      at.pages.set(page, kept)
    } else {
      void loadPage(path, page)
    }
  }
}

/** The row `index` of the graph of `path`, if it has been read. */
export function rowAt(path: string, index: number): GraphRow | undefined {
  return held.get(path)?.pages.get(Math.floor(index / PAGE))?.[index % PAGE]
}

/** The row of the commit `id`, laid out as far as it takes; `null` when the
 * graph does not hold it. */
export async function positionOf(path: string, id: string): Promise<number | null> {
  const state = held.get(path)?.view.state
  if (!state) return null
  try {
    return await api.graphPosition(path, state.generation, id)
  } catch {
    return null
  }
}

/** Searches the graph of `path` for `query`, and keeps what is found in its
 * view as it arrives; `first` hears the rows of the first answer that found
 * any. An empty query ends the search. */
export async function searchGraph(path: string, query: string, first?: (rows: number[]) => void): Promise<void> {
  const at = entry(path)
  const state = at.view.state
  at.searchTicket += 1
  const ticket = at.searchTicket
  if (!state || query.trim() === '') {
    update(path, { search: null })
    return
  }
  const found = new Set<number>()
  const byGraph = new Set<number>()
  const byNooma = new Set<number>()
  const searching = new Set<Finder>()
  const failed: string[] = []
  let heard = false
  const publish = () => {
    if (at.searchTicket !== ticket) return
    const rows = [...found].sort((a, b) => a - b)
    if (!heard && rows.length > 0) {
      heard = true
      first?.(rows)
    }
    const inMessage = new Set([...byNooma].filter((row) => !byGraph.has(row)))
    update(path, {
      search: { query, rows, inMessage, searching: new Set(searching), failed: failed.length ? failed.join('\n') : null },
    })
  }

  const channel = new Channel<Found>()
  channel.onmessage = (message) => {
    if (at.searchTicket !== ticket) return
    if (message.kind === 'rows') {
      for (const row of message.rows) {
        found.add(row)
        ;(message.by === 'graph' ? byGraph : byNooma).add(row)
      }
    } else if (message.kind === 'done') {
      searching.delete(message.by)
    } else {
      searching.delete(message.by)
      failed.push(message.reason)
    }
    publish()
  }
  try {
    for (const finder of await api.graphFind(path, state.generation, query, channel)) searching.add(finder)
  } catch (cause) {
    failed.push(failure(cause))
  }
  publish()
}

const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

/** The graph of `path`, opened on first use. */
export function useGraph(path: string): GraphView {
  useEffect(() => {
    if (held.get(path)?.view.state == null) void openGraph(path)
  }, [path])
  return useSyncExternalStore(subscribe, () => held.get(path)?.view ?? NOTHING)
}
