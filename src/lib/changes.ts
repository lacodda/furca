import type { FileChange } from './generated/FileChange'
import type { Hunk } from './generated/Hunk'
import type { Side } from './generated/Side'
import { nameTree, type NameNode } from './names'

/*
 * The sums behind a commit's details, with no React in them: the tree its
 * files are listed in, what the column beside each says, and the core's
 * hunks as the editor's changes.
 */

/** A folder of changed files, or a file. A folder that holds one folder
 * and nothing else is drawn as one row, `src/lib`, as Fork and GitHub do:
 * a deep path is otherwise a staircase of rows with nothing to choose. */
export interface FileNode {
  id: string
  label: string
  file?: FileChange
  children: FileNode[]
}

/** The changed files as a tree of folders, folders first, by path. */
export function fileTree(files: readonly FileChange[]): FileNode[] {
  const compact = (node: NameNode<FileChange>): FileNode => {
    let label = node.label
    let current = node
    while (current.item === undefined && current.children.length === 1 && current.children[0]!.children.length > 0) {
      current = current.children[0]!
      label = `${label}/${current.label}`
    }
    return { id: current.id, label, file: current.item, children: current.children.map(compact) }
  }
  return nameTree(files, (file) => file.path).map(compact)
}

/** Every folder of a tree, so all of them can start open. */
export function folderIds(nodes: readonly FileNode[]): string[] {
  return nodes.flatMap((node) => (node.children.length > 0 ? [node.id, ...folderIds(node.children)] : []))
}

/** Bytes as a person reads them: `812 B`, `24.1 KB`, `1.3 MB`. Powers of
 * 1024, as the file managers people compare it with count. */
export function sizeText(bytes: number): string {
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return unit === 0 ? `${String(value)} B` : `${value.toFixed(1)} ${units[unit]!}`
}

/** What the column beside a file says when it is not plain text: a binary
 * or a git LFS file with its size, or a submodule. `null` for text. */
export interface Stored {
  kind: 'binary' | 'lfs' | 'submodule'
  before: number | null
  after: number | null
}

export function storedAs(file: FileChange): Stored | null {
  const sides = [file.before, file.after].filter((side): side is Side => side !== null)
  const kind = sides.find((side) => side.content.kind !== 'text')?.content.kind
  if (kind === undefined || kind === 'text') return null
  const size = (side: Side | null) => (side !== null && side.content.kind === kind ? side.size : null)
  return { kind, before: size(file.before), after: size(file.after) }
}

/** Whether a file is a binary committed as it is - not through git LFS - on
 * either side: the change a public repository regrets. */
export function isRawBinary(file: FileChange): boolean {
  return [file.before, file.after].some((side) => side?.content.kind === 'binary')
}

/** The line endings of a text that changed them, `CRLF → LF`; `null` when
 * they did not change or a side is not text. */
export function endingsChange(file: FileChange): { before: string; after: string } | null {
  const ending = (side: Side | null) => (side?.content.kind === 'text' ? side.content.endings : null)
  const before = ending(file.before)
  const after = ending(file.after)
  if (before === null || after === null || before === after) return null
  const name = { lf: 'LF', crlf: 'CRLF', mixed: 'mixed', 'no-ending': 'none' } as const
  return { before: name[before], after: name[after] }
}

/** A change between two texts by character offsets, as the editor's diff
 * reports it. */
export interface CharChange {
  fromA: number
  toA: number
  fromB: number
  toB: number
}

/** Where each line of `text` ends, before its `\n`. */
function lineEnds(text: string): number[] {
  const ends: number[] = []
  let at = text.indexOf('\n')
  while (at !== -1) {
    ends.push(at)
    at = text.indexOf('\n', at + 1)
  }
  if (text.length > 0 && !text.endsWith('\n')) ends.push(text.length)
  return ends
}

/**
 * The core's hunks as character ranges of the two texts - so the editor
 * shows the lines git shows, not ones it works out again.
 *
 * A run of lines is taken with the newline before it rather than the one
 * after, which keeps a file that does not end in a newline in step; where
 * the two texts still differ between two ranges - only ever the newline at
 * the very end - that gap joins the range before it.
 */
export function hunkChanges(a: string, b: string, hunks: readonly Hunk[]): CharChange[] {
  const endsA = lineEnds(a)
  const endsB = lineEnds(b)
  const span = (ends: number[], start: number, count: number): [number, number] => {
    if (start === 0) return [0, count === 0 ? 0 : (ends[count - 1] ?? 0) + 1]
    const from = ends[start - 1] ?? 0
    return [from, count === 0 ? from : (ends[start + count - 1] ?? from)]
  }
  const raw = hunks.map((hunk) => {
    const [fromA, toA] = span(endsA, hunk.before.start, hunk.before.count)
    const [fromB, toB] = span(endsB, hunk.after.start, hunk.after.count)
    return { fromA: Math.min(fromA, a.length), toA: Math.min(toA, a.length), fromB: Math.min(fromB, b.length), toB: Math.min(toB, b.length) }
  })
  const out: CharChange[] = []
  let atA = 0
  let atB = 0
  for (const change of [...raw, { fromA: a.length, toA: a.length, fromB: b.length, toB: b.length }]) {
    const same = a.slice(atA, change.fromA) === b.slice(atB, change.fromB)
    const last = out.at(-1)
    if (!same && last !== undefined) {
      last.toA = change.toA
      last.toB = change.toB
    } else if (!same) {
      out.push({ fromA: atA, toA: change.toA, fromB: atB, toB: change.toB })
    } else if (change.fromA !== change.toA || change.fromB !== change.toB) {
      out.push({ ...change })
    }
    atA = Math.max(atA, change.toA)
    atB = Math.max(atB, change.toB)
  }
  return out
}

/** Within each change, the characters that differ, from `inner` - the
 * editor's own character diff, run on the lines of one hunk at a time
 * rather than on the whole file. A hunk too large to compare that finely
 * stays one change. */
export function refine(
  a: string,
  b: string,
  changes: readonly CharChange[],
  inner: (a: string, b: string) => readonly CharChange[],
  most = 20_000,
): CharChange[] {
  return changes.flatMap((change) => {
    const lengthA = change.toA - change.fromA
    const lengthB = change.toB - change.fromB
    if (lengthA === 0 || lengthB === 0 || lengthA > most || lengthB > most) return [change]
    return inner(a.slice(change.fromA, change.toA), b.slice(change.fromB, change.toB)).map((part) => ({
      fromA: part.fromA + change.fromA,
      toA: part.toA + change.fromA,
      fromB: part.fromB + change.fromB,
      toB: part.toB + change.fromB,
    }))
  })
}
