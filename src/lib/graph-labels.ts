import type { Sidebar } from './generated/Sidebar'
import { byVersionDescending } from './names'

/*
 * What the graph writes beside a commit, and which match a search moves to:
 * sums with no React in them, tested on their own.
 */

/** A name pointing at a commit, as a row shows it. */
export interface Label {
  name: string
  /** `head` is the branch `HEAD` is on, or `HEAD` itself when detached. */
  kind: 'head' | 'branch' | 'remote' | 'tag' | 'stash'
}

const ORDER: Record<Label['kind'], number> = { head: 0, branch: 1, remote: 2, tag: 3, stash: 4 }

/** Every commit something points at, with what points at it: the branch
 * `HEAD` is on first, then branches, remote-tracking branches, tags and
 * stash entries, each kind in the order the column lists it. A stash entry
 * is a commit git keeps under `refs/stash`, and the graph of every ref draws
 * it as `git log --all` does. */
export function labelsByCommit(sidebar: Sidebar): Map<string, Label[]> {
  const labels = new Map<string, Label[]>()
  const add = (commit: string, label: Label) => {
    labels.set(commit, [...(labels.get(commit) ?? []), label])
  }
  if (sidebar.head.detached && sidebar.head.commit !== null) {
    add(sidebar.head.commit, { name: 'HEAD', kind: 'head' })
  }
  for (const branch of sidebar.refs.branches) {
    add(branch.target, { name: branch.name, kind: branch.head ? 'head' : 'branch' })
  }
  for (const branch of sidebar.refs.remote_branches) {
    add(branch.target, { name: branch.name, kind: 'remote' })
  }
  const tags = [...sidebar.refs.tags].sort((a, b) => byVersionDescending(a.name, b.name))
  for (const tag of tags) add(tag.target, { name: tag.name, kind: 'tag' })
  for (const stash of sidebar.stashes) add(stash.commit, { name: `stash@{${String(stash.index)}}`, kind: 'stash' })
  // A sort is stable: within a kind, the order above stays.
  for (const list of labels.values()) list.sort((a, b) => ORDER[a.kind] - ORDER[b.kind])
  return labels
}

/** Where a search moves from `selected`: the next match below it, or the one
 * above it going `back`, coming round at either end. `null` when nothing
 * matched. */
export function nextMatch(matches: readonly number[], selected: number | null, back: boolean): number | null {
  if (matches.length === 0) return null
  if (selected === null) return back ? (matches.at(-1) ?? null) : (matches[0] ?? null)
  if (back) {
    for (let at = matches.length - 1; at >= 0; at--) {
      const row = matches[at]!
      if (row < selected) return row
    }
    return matches.at(-1) ?? null
  }
  for (const row of matches) if (row > selected) return row
  return matches[0] ?? null
}

/** Which match `selected` is, counted from 1; 0 when it is none of them. */
export function matchNumber(matches: readonly number[], selected: number | null): number {
  if (selected === null) return 0
  // Matches arrive sorted: a halving search, not a scan of thousands.
  let low = 0
  let high = matches.length - 1
  while (low <= high) {
    const middle = (low + high) >> 1
    const row = matches[middle]!
    if (row === selected) return middle + 1
    if (row < selected) low = middle + 1
    else high = middle - 1
  }
  return 0
}

/** The commit a row of the left column names: a branch, a remote-tracking
 * branch or a tag, by the id the column gave the row. */
export function commitOfColumnRow(sidebar: Sidebar, id: string): string | null {
  if (id.startsWith('b:')) {
    const name = id.slice(2)
    return sidebar.refs.branches.find((branch) => branch.name === name)?.target ?? null
  }
  if (id.startsWith('t:')) {
    const name = id.slice(2)
    return sidebar.refs.tags.find((tag) => tag.name === name)?.target ?? null
  }
  if (id.startsWith('s:')) {
    const index = Number(id.slice(2))
    return sidebar.stashes.find((stash) => stash.index === index)?.commit ?? null
  }
  if (id.startsWith('r:')) {
    // `r:origin:feature/x` is origin/feature/x; `r:origin` is the remote.
    const rest = id.slice(2)
    const colon = rest.indexOf(':')
    if (colon < 0) return null
    const name = `${rest.slice(0, colon)}/${rest.slice(colon + 1)}`
    return sidebar.refs.remote_branches.find((branch) => branch.name === name)?.target ?? null
  }
  return null
}

/** A piece of text, and whether it is a place the search found. */
export interface Piece {
  text: string
  marked: boolean
}

/** `text` cut into pieces at every place `query` occurs in it, compared
 * without case. A text whose lowercase is not as long as itself - a few
 * letters lowercase into two - is left whole rather than marked off by a
 * letter. */
export function markedPieces(text: string, query: string): Piece[] {
  const words = query.trim().toLowerCase()
  const lower = text.toLowerCase()
  if (words === '' || lower.length !== text.length) return [{ text, marked: false }]
  const pieces: Piece[] = []
  let at = 0
  for (let found = lower.indexOf(words); found >= 0; found = lower.indexOf(words, at)) {
    if (found > at) pieces.push({ text: text.slice(at, found), marked: false })
    pieces.push({ text: text.slice(found, found + words.length), marked: true })
    at = found + words.length
  }
  if (at < text.length) pieces.push({ text: text.slice(at), marked: false })
  return pieces
}
