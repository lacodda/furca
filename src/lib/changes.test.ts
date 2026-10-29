import { describe, expect, it } from 'vitest'
import type { FileChange } from './generated/FileChange'
import type { Hunk } from './generated/Hunk'
import type { Side } from './generated/Side'
import { endingsChange, fileTree, folderIds, hunkChanges, isRawBinary, refine, sizeText, storedAs, type CharChange } from './changes'

const text = (size: number, endings: 'lf' | 'crlf' = 'lf'): Side => ({
  id: 'a'.repeat(40),
  mode: 'file',
  content: { kind: 'text', endings },
  size,
})

const change = (path: string, overrides: Partial<FileChange> = {}): FileChange => ({
  path,
  status: 'modified',
  from: null,
  similarity: null,
  before: text(10),
  after: text(12),
  lines: { added: 1, removed: 1 },
  ...overrides,
})

/** `a` with every change put in: what `b` must be when the changes are
 * whole and in step. */
function apply(a: string, b: string, changes: readonly CharChange[]): string {
  let out = ''
  let at = 0
  for (const c of changes) {
    out += a.slice(at, c.fromA) + b.slice(c.fromB, c.toB)
    at = c.toA
  }
  return out + a.slice(at)
}

const hunk = (beforeStart: number, beforeCount: number, afterStart: number, afterCount: number): Hunk => ({
  before: { start: beforeStart, count: beforeCount },
  after: { start: afterStart, count: afterCount },
})

describe('fileTree', () => {
  it('folds a folder that only holds a folder into one row', () => {
    const tree = fileTree([change('src/lib/a.ts'), change('src/lib/b.ts'), change('README.md')])
    expect(tree.map((node) => node.label)).toEqual(['src/lib', 'README.md'])
    expect(tree[0]!.children.map((node) => node.file?.path)).toEqual(['src/lib/a.ts', 'src/lib/b.ts'])
    expect(folderIds(tree)).toEqual(['src/lib'])
  })

  it('keeps a folder that holds files and folders as it is', () => {
    const tree = fileTree([change('src/a.ts'), change('src/deep/er/b.ts')])
    expect(tree.map((node) => node.label)).toEqual(['src'])
    expect(tree[0]!.children.map((node) => node.label)).toEqual(['deep/er', 'a.ts'])
  })
})

describe('what the column beside a file says', () => {
  it('prints sizes as a file manager does', () => {
    expect(sizeText(812)).toBe('812 B')
    expect(sizeText(24_576)).toBe('24.0 KB')
    expect(sizeText(126_418_944)).toBe('120.6 MB')
  })

  it('names a binary, a git LFS file and a submodule, and warns of a binary only', () => {
    const binary = change('logo.png', { before: null, after: { ...text(2048), content: { kind: 'binary' } }, lines: null })
    expect(storedAs(binary)).toEqual({ kind: 'binary', before: null, after: 2048 })
    expect(isRawBinary(binary)).toBe(true)

    const lfs = change('intro.mp4', {
      after: { ...text(126_418_944), content: { kind: 'lfs', oid: 'f'.repeat(64) } },
      before: null,
      lines: null,
    })
    expect(storedAs(lfs)?.kind).toBe('lfs')
    expect(isRawBinary(lfs)).toBe(false)

    expect(storedAs(change('notes.txt'))).toBeNull()
  })

  it('says when the line endings changed, and only then', () => {
    expect(endingsChange(change('a.txt', { before: text(10, 'crlf'), after: text(8) }))).toEqual({ before: 'CRLF', after: 'LF' })
    expect(endingsChange(change('a.txt'))).toBeNull()
    expect(endingsChange(change('a.txt', { before: null }))).toBeNull()
  })
})

describe('hunkChanges', () => {
  it('turns lines changed in the middle into the characters of those lines', () => {
    const a = 'one\ntwo\nthree\n'
    const b = 'one\n2\nthree\n'
    const changes = hunkChanges(a, b, [hunk(1, 1, 1, 1)])
    expect(changes).toEqual([{ fromA: 3, toA: 7, fromB: 3, toB: 5 }])
    expect(apply(a, b, changes)).toBe(b)
  })

  it('stays in step at the start, at the end and around a missing final newline', () => {
    const cases: [string, string, Hunk[]][] = [
      ['x\n', 'y\nx\n', [hunk(0, 0, 0, 1)]],
      ['a\nb\n', 'a\n', [hunk(1, 1, 1, 0)]],
      ['x', 'x\ny', [hunk(1, 0, 1, 1)]],
      ['x\n', 'x\ny', [hunk(1, 0, 1, 1)]],
      ['a\nb', 'a\nc', [hunk(1, 1, 1, 1)]],
      ['', 'new\nfile\n', [hunk(0, 0, 0, 2)]],
      ['a\nb\nc\nd\n', 'a\nB\nc\nd\ne', [hunk(1, 1, 1, 1), hunk(4, 0, 4, 1)]],
    ]
    for (const [a, b, hunks] of cases) {
      const changes = hunkChanges(a, b, hunks)
      expect(apply(a, b, changes), JSON.stringify([a, b])).toBe(b)
      for (const c of changes) {
        expect(c.fromA <= c.toA && c.fromB <= c.toB).toBe(true)
      }
    }
  })

  it('marks only the lines git marks', () => {
    const a = 'a\nb\nc\nd\ne\nf\n'
    const b = 'a\nB\nc\nd\nE\nf\n'
    const changes = hunkChanges(a, b, [hunk(1, 1, 1, 1), hunk(4, 1, 4, 1)])
    expect(changes).toHaveLength(2)
    expect(changes.map((c) => b.slice(c.fromB, c.toB))).toEqual(['\nB', '\nE'])
  })

  it('narrows each change to the characters that differ, and keeps a whole one whole', () => {
    const a = 'let value = weigh(1)\n'
    const b = 'let value = weigh(7)\n'
    const whole = hunkChanges(a, b, [hunk(0, 1, 0, 1)])
    const inner = (x: string, y: string) => [{ fromA: x.indexOf('1'), toA: x.indexOf('1') + 1, fromB: y.indexOf('7'), toB: y.indexOf('7') + 1 }]
    const fine = refine(a, b, whole, inner)
    expect(fine).toEqual([{ fromA: 18, toA: 19, fromB: 18, toB: 19 }])
    expect(apply(a, b, fine)).toBe(b)
    expect(refine(a, b, whole, inner, 4)).toEqual(whole)
  })
})
