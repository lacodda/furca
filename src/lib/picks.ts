import type { Hunk } from './generated/Hunk'
import type { Picked } from './generated/Picked'

/*
 * Lines picked out of a change, to stage, unstage or discard on their own. A
 * line is picked on the side it is on: a removed line in the text before, an
 * added one in the text after, each counted from 0 as the hunks count them.
 * Only a changed line can be picked; the core takes them per hunk
 * (`Repository::stage_lines`).
 */

/** Which text a line is counted in. */
export type Side = 'before' | 'after'

/** The lines picked on each side. */
export interface Picks {
  before: ReadonlySet<number>
  after: ReadonlySet<number>
}

export const NO_PICKS: Picks = { before: new Set(), after: new Set() }

function inside(hunk: Hunk, side: Side, line: number): boolean {
  const range = hunk[side]
  return line >= range.start && line < range.start + range.count
}

/** Whether `line` of `side` is one a hunk changed, and so can be picked. */
export function isChanged(hunks: readonly Hunk[], side: Side, line: number): boolean {
  return hunks.some((hunk) => inside(hunk, side, line))
}

/** How many lines are picked. */
export function pickCount(picks: Picks): number {
  return picks.before.size + picks.after.size
}

/** `picks` with every changed line of `side` from `from` to `to`, either way
 * round and both included, picked when `on` and let go otherwise. */
export function setLines(picks: Picks, hunks: readonly Hunk[], side: Side, from: number, to: number, on: boolean): Picks {
  const lines = new Set(picks[side])
  for (let line = Math.min(from, to); line <= Math.max(from, to); line++) {
    if (!isChanged(hunks, side, line)) continue
    if (on) lines.add(line)
    else lines.delete(line)
  }
  return { ...picks, [side]: lines }
}

/** The picks that are still changed lines of `hunks`: a file read again may
 * have moved, and a pick it no longer has is let go rather than sent. */
export function stillPicked(picks: Picks, hunks: readonly Hunk[]): Picks {
  const keep = (side: Side) => new Set([...picks[side]].filter((line) => isChanged(hunks, side, line)))
  const next = { before: keep('before'), after: keep('after') }
  return next.before.size === picks.before.size && next.after.size === picks.after.size ? picks : next
}

/** The picks as the core takes them: each hunk with a line picked, and its
 * lines that are. */
export function pickedOf(picks: Picks, hunks: readonly Hunk[]): Picked[] {
  const out: Picked[] = []
  for (const hunk of hunks) {
    const removed = [...picks.before].filter((line) => inside(hunk, 'before', line)).sort((a, b) => a - b)
    const added = [...picks.after].filter((line) => inside(hunk, 'after', line)).sort((a, b) => a - b)
    if (removed.length + added.length > 0) out.push({ hunk, removed, added })
  }
  return out
}

/** A hunk taken whole. */
export function wholeHunk(hunk: Hunk): Picked {
  const all = (side: Side) => Array.from({ length: hunk[side].count }, (_, at) => hunk[side].start + at)
  return { hunk, removed: all('before'), added: all('after') }
}
