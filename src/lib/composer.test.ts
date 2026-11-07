import { describe, expect, it } from 'vitest'
import { afterCommit, canDraft, compose, EMPTY, fromParts, grouped, whyNot, type Draft } from './composer'

const draft = (over: Partial<Draft>): Draft => ({ ...EMPTY, description: 'a thing', ...over })

describe('compose', () => {
  it('writes the subject of the convention from its fields', () => {
    expect(compose(draft({ kind: 'feat' }))).toBe('feat: a thing')
    expect(compose(draft({ kind: 'fix', scope: 'core' }))).toBe('fix(core): a thing')
    expect(compose(draft({ kind: 'fix', scope: 'core', breaking: true }))).toBe('fix(core)!: a thing')
    expect(compose(draft({ kind: 'fix', breaking: true }))).toBe('fix!: a thing')
  })

  it('leaves a subject with no type as it was typed', () => {
    expect(compose(draft({ kind: '', scope: 'ignored', breaking: true, description: 'Tidy up' }))).toBe('Tidy up')
  })

  it('puts a blank line before a body, and no trailing space anywhere', () => {
    expect(compose(draft({ kind: 'feat', description: '  a thing ', body: '\nWhy it was done.\n\n' }))).toBe('feat: a thing\n\nWhy it was done.')
    expect(compose(draft({ kind: 'feat', scope: '  ', body: '   ' }))).toBe('feat: a thing')
  })
})

describe('fromParts', () => {
  it('shows a message out of the convention as a form with no type', () => {
    const parts = { kind: null, scope: null, breaking: false, description: 'Tidy up', body: 'More.' }
    expect(fromParts(parts)).toEqual({ kind: '', scope: '', breaking: false, description: 'Tidy up', body: 'More.' })
    expect(compose(fromParts(parts))).toBe('Tidy up\n\nMore.')
  })

  it('comes back as the same message it was taken apart from', () => {
    const parts = { kind: 'feat', scope: 'cli', breaking: true, description: 'drop the flag', body: 'Why.\n\nBREAKING CHANGE: gone' }
    expect(compose(fromParts(parts))).toBe('feat(cli)!: drop the flag\n\nWhy.\n\nBREAKING CHANGE: gone')
  })
})

describe('afterCommit', () => {
  it('keeps the kind of work and clears the rest', () => {
    expect(afterCommit(draft({ kind: 'fix', scope: 'core', breaking: true, body: 'x' }))).toEqual({
      kind: 'fix',
      scope: '',
      breaking: false,
      description: '',
      body: '',
    })
  })
})

describe('whyNot', () => {
  it('stops only at what git would refuse', () => {
    expect(whyNot(draft({}), 1, 0, false, false)).toBeNull()
    expect(whyNot(draft({}), 0, 0, false, false)).toBe('nothing-staged')
    expect(whyNot(draft({ description: '   ' }), 1, 0, false, false)).toBe('no-description')
    expect(whyNot(draft({}), 1, 2, false, false)).toBe('conflicted')
    expect(whyNot(draft({}), 1, 0, false, true)).toBe('busy')
  })

  it('lets an amend change only the message', () => {
    expect(whyNot(draft({}), 0, 0, true, false)).toBeNull()
    expect(whyNot(draft({ description: '' }), 0, 0, true, false)).toBe('no-description')
  })
})

describe('canDraft', () => {
  it('never fills a form someone has typed in', () => {
    expect(canDraft(EMPTY)).toBe(true)
    expect(canDraft({ ...EMPTY, kind: 'fix', scope: 'core' })).toBe(true)
    expect(canDraft(draft({}))).toBe(false)
    expect(canDraft({ ...EMPTY, body: 'x' })).toBe(false)
  })
})

describe('grouped', () => {
  it('gathers the doctor by kind, the assistant first', () => {
    expect(
      grouped([
        { kind: 'endings', path: 'a.txt', before: 'lf', after: 'crlf' },
        { kind: 'assistant', path: 'CLAUDE.md' },
        { kind: 'endings', path: 'b.txt', before: null, after: 'crlf' },
      ]),
    ).toEqual([
      { kind: 'assistant', paths: ['CLAUDE.md'] },
      { kind: 'endings', paths: ['a.txt', 'b.txt'] },
    ])
    expect(grouped([])).toEqual([])
  })
})
