import type { Block } from './generated/Block'
import type { Conflict } from './generated/Conflict'
import type { Halfway } from './generated/Halfway'
import type { LineRange } from './generated/LineRange'

/*
 * What the resolver does with a conflict's blocks, kept apart from the
 * editors so it can be read and tested on its own.
 *
 * The result starts as the file would be with every block that goes in on its
 * own already in, and every conflict written out between git's markers - in
 * the diff3 style, with the ancestor's lines - as a person knows them from a
 * file git left. Choosing a side for a conflict replaces its markers with that
 * side's lines; editing by hand does the same. A conflict is resolved once no
 * markers are left in it, and the file once none are left anywhere: the
 * markers are the one truth of what is still to do.
 */

/** What a conflicting block takes. */
export type Pick = 'ours' | 'theirs' | 'both' | 'base'

/** The three texts as the resolver shows them: a `\r` before a line's end is
 * left off, as a diff shows it; the core gives it back when it writes. */
export interface Shown {
  base: string
  ours: string
  theirs: string
}

/** Where a conflict's markers are in the result, as offsets. */
export interface Span {
  from: number
  to: number
}

/** The names the markers carry: the two sides as a person knows them. */
export interface Labels {
  ours: string
  theirs: string
}

/** `text` with a `\r` before each line's end left off. */
export function withoutCr(text: string): string {
  return text.replace(/\r(?=\n|$)/g, '')
}

export function shownTexts(conflict: Conflict): Shown | null {
  if (conflict.texts === null) return null
  return {
    base: withoutCr(conflict.texts.base),
    ours: withoutCr(conflict.texts.ours),
    theirs: withoutCr(conflict.texts.theirs),
  }
}

/** The lines of `text`, each with its `\n`. */
export function linesOf(text: string): string[] {
  const out: string[] = []
  let at = 0
  while (at < text.length) {
    const end = text.indexOf('\n', at)
    const next = end === -1 ? text.length : end + 1
    out.push(text.slice(at, next))
    at = next
  }
  return out
}

/** Text put together a piece at a time. A piece after a last line with no
 * newline is given one first: two lines must not run into one. */
class Builder {
  text = ''

  add(piece: string): void {
    if (piece === '') return
    if (this.text !== '' && !this.text.endsWith('\n')) this.text += '\n'
    this.text += piece
  }
}

function lines(of: string[], range: LineRange): string {
  return of.slice(range.start, range.start + range.count).join('')
}

/** A text that ends where a line does, so a marker can follow it. */
function closed(text: string): string {
  return text === '' || text.endsWith('\n') ? text : `${text}\n`
}

/** The lines of one side, split once for every block. */
interface Split {
  base: string[]
  ours: string[]
  theirs: string[]
}

function split(shown: Shown): Split {
  return { base: linesOf(shown.base), ours: linesOf(shown.ours), theirs: linesOf(shown.theirs) }
}

/** What a block goes in as: its own lines, for a block that goes in on its
 * own; for a conflict, the side `pick` takes. */
function blockText(block: Block, of: Split, pick: Pick): string {
  switch (block.kind) {
    case 'theirs':
      return lines(of.theirs, block.theirs)
    case 'conflict': {
      if (pick === 'theirs') return lines(of.theirs, block.theirs)
      if (pick === 'base') return lines(of.base, block.base)
      if (pick === 'both') {
        const both = new Builder()
        both.add(lines(of.ours, block.ours))
        both.add(lines(of.theirs, block.theirs))
        return both.text
      }
      return lines(of.ours, block.ours)
    }
    default:
      return lines(of.ours, block.ours)
  }
}

/** A conflict written out as git writes it in the diff3 style. */
export function marked(block: Block, of: Split, labels: Labels): string {
  return [
    `<<<<<<< ${labels.ours}\n`,
    closed(lines(of.ours, block.ours)),
    '||||||| base\n',
    closed(lines(of.base, block.base)),
    '=======\n',
    closed(lines(of.theirs, block.theirs)),
    `>>>>>>> ${labels.theirs}\n`,
  ].join('')
}

/** The result as the resolver starts it, and where each conflict is in it. */
export function startingResult(conflict: Conflict, shown: Shown, labels: Labels): { text: string; spans: Span[] } {
  const of = split(shown)
  const out = new Builder()
  const spans: Span[] = []
  for (const block of conflict.blocks) {
    if (block.kind === 'conflict') {
      // The newline a piece may be given comes first, outside the span.
      if (out.text !== '' && !out.text.endsWith('\n')) out.text += '\n'
      const from = out.text.length
      out.add(marked(block, of, labels))
      spans.push({ from, to: out.text.length })
    } else {
      out.add(blockText(block, of, 'ours'))
    }
  }
  return { text: out.text, spans }
}

/** What the conflict `index` - counted among the conflicts alone - takes
 * from `pick`. */
export function pickedText(conflict: Conflict, shown: Shown, index: number, pick: Pick): string {
  const block = conflict.blocks.filter((each) => each.kind === 'conflict')[index]
  if (block === undefined) return ''
  return blockText(block, split(shown), pick)
}

/** The line, from 1, where `text` still holds git's markers: the first
 * `<<<<<<<` that a `>>>>>>>` closes further down. A line of `=======` alone is
 * not one - it underlines a Markdown heading as often as it parts a conflict.
 * The same rule as the core's. */
export function markersIn(text: string): number | null {
  const marker = (line: string, sign: string) =>
    line.startsWith(sign.repeat(7)) && (line.length === 7 || line[7] === ' ' || line[7] === '\t' || line[7] === '\r')
  let opened: number | null = null
  const all = text.split('\n')
  for (let at = 0; at < all.length; at++) {
    const line = all[at] ?? ''
    if (opened === null && marker(line, '<')) opened = at + 1
    else if (opened !== null && marker(line, '>')) return opened
  }
  return null
}

/** How many conflicts a file has. */
export function conflictCount(conflict: Conflict): number {
  return conflict.blocks.filter((block) => block.kind === 'conflict').length
}

/** The offsets each line of `text` starts at, and its end. */
export function lineStarts(text: string): number[] {
  const starts = [0]
  for (let at = text.indexOf('\n'); at !== -1; at = text.indexOf('\n', at + 1)) starts.push(at + 1)
  if (starts.at(-1) !== text.length) starts.push(text.length)
  return starts
}

/** A run of lines as offsets into a text whose lines start at `starts`. */
export function offsets(starts: number[], range: LineRange): Span {
  const end = starts.at(-1) ?? 0
  return {
    from: starts[range.start] ?? end,
    to: starts[range.start + range.count] ?? end,
  }
}

/** The name a merge's message gives what it merges: `feature` in
 * `Merge branch 'feature'`; `null` when it gives none. */
export function mergedName(message: string | null): string | null {
  if (message === null) return null
  const quoted = /'([^']+)'/.exec(message)
  return quoted?.[1] ?? null
}

/** Whether a conflict's sides are taken whole rather than in blocks: a file
 * one side deleted, a binary. */
export function takenWhole(conflict: Conflict): boolean {
  return conflict.blocks.length === 0
}

/** What the window says the operation brings in, short: the name a merge
 * merges, or the commit a pick, a revert or a rebase is on. */
export function bringing(halfway: Halfway): string | null {
  if (halfway.operation === 'merge') {
    const name = mergedName(halfway.message)
    if (name !== null) return name
  }
  return halfway.commit === null ? null : halfway.commit.id.slice(0, 7)
}
