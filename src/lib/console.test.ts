import { describe, expect, it } from 'vitest'
import type { GitRun } from './generated/GitRun'
import { commandLine, failed, withRun } from './console'

const run = (args: string[], code: number | null = 0): GitRun => ({ args, code, millis: 3, at: 0, input: '', said: '' })

describe('commandLine', () => {
  it('reads as it would be typed', () => {
    expect(commandLine(run(['add', '--all']))).toBe('git add --all')
    expect(commandLine(run(['tag', '--message', 'a "b" c', '']))).toBe('git tag --message "a \\"b\\" c" ""')
  })
})

describe('failed', () => {
  it('is any end but a clean one, a signal among them', () => {
    expect(failed(run(['status']))).toBe(false)
    expect(failed(run(['status'], 1))).toBe(true)
    expect(failed(run(['status'], null))).toBe(true)
  })
})

describe('withRun', () => {
  it('adds at the end, and lets go of the oldest past what is kept', () => {
    const one = withRun([], run(['one']))
    expect(withRun(one, run(['two'])).map((each) => each.args[0])).toEqual(['one', 'two'])
    const full = Array.from({ length: 500 }, (_, index) => run([String(index)]))
    const after = withRun(full, run(['last']))
    expect(after).toHaveLength(500)
    expect(after[0]?.args[0]).toBe('1')
    expect(after.at(-1)?.args[0]).toBe('last')
  })
})
