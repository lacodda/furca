import { describe, expect, it } from 'vitest'
import type { Check } from './generated/Check'
import type { Heard } from './generated/Heard'
import type { Plan } from './generated/Plan'
import type { ReleaseState } from './generated/ReleaseState'
import type { Report } from './generated/Report'
import type { RunStep } from './generated/RunStep'
import { allowed, atTheEnd, beginning, chosenOrShown, firstToShow, fromUnderWay, hearing, linesOfStep, MOST_LINES, stepToShow, type Seen } from './release'

const NAMES = ['plan', 'gate', 'commit']
const word = (name: string, status: RunStep['status'], summary = ''): RunStep => ({ name, status, summary, details: [] })
const began = (step: string): Heard => ({ kind: 'told', told: { kind: 'began', step } })
const said = (step: string, line: string): Heard => ({ kind: 'told', told: { kind: 'said', step, line } })
const told = (step: RunStep): Heard => ({ kind: 'told', told: { kind: 'ended', step } })
const printed = (line: string): Heard => ({ kind: 'printed', line })
const step = (seen: Seen, name: string) => seen.steps.find((each) => each.name === name)

const report = (steps: RunStep[], released: boolean): Report => ({
  project: 'demo',
  version: '1.2.0',
  tag: 'v1.2.0',
  released,
  steps,
  commit: null,
  registries: [],
  page: null,
  git: [],
})

describe('hearing a run', () => {
  it('starts with every step ahead', () => {
    const seen = beginning(NAMES)
    expect(seen.steps.map((each) => each.status)).toEqual(['ahead', 'ahead', 'ahead'])
    expect(seen.current).toBeNull()
    expect(seen.ended).toBe(false)
  })

  it('follows a step from beginning to ending', () => {
    let seen = hearing(beginning(NAMES), [began('plan')])
    expect(step(seen, 'plan')?.status).toBe('running')
    expect(seen.current).toBe('plan')
    seen = hearing(seen, [told(word('plan', 'done', 'demo 1.1.0 -> 1.2.0'))])
    expect(step(seen, 'plan')?.status).toBe('done')
    expect(step(seen, 'plan')?.summary).toBe('demo 1.1.0 -> 1.2.0')
    expect(seen.current).toBeNull()
    expect(step(seen, 'gate')?.status).toBe('ahead')
  })

  it('puts what is printed in the step going on, in order, with what it said', () => {
    const seen = hearing(beginning(NAMES), [began('gate'), said('gate', 'cargo test'), printed('running 3 tests'), printed('ok')])
    expect(step(seen, 'gate')?.lines).toEqual(['cargo test', 'running 3 tests', 'ok'])
    expect(step(seen, 'plan')?.lines).toEqual([])
  })

  it('keeps the lines of a step once it has ended', () => {
    const seen = hearing(beginning(NAMES), [began('gate'), printed('ok'), told(word('gate', 'done', '8 commands passed'))])
    expect(step(seen, 'gate')?.lines).toEqual(['ok'])
    expect(step(seen, 'gate')?.summary).toBe('8 commands passed')
  })

  it('has nowhere to put what is printed between steps, and loses nothing else', () => {
    const seen = hearing(beginning(NAMES), [printed('stray')])
    expect(seen.steps.flatMap((each) => each.lines)).toEqual([])
  })

  it('keeps the newest lines of a step, and says how many went', () => {
    const lines = Array.from({ length: MOST_LINES + 5 }, (_, index) => printed(String(index)))
    const shown = step(hearing(beginning(NAMES), [began('gate'), ...lines]), 'gate')
    expect(shown?.lines).toHaveLength(MOST_LINES)
    expect(shown?.lines[0]).toBe('5')
    expect(shown?.lines.at(-1)).toBe(String(MOST_LINES + 4))
    expect(shown?.cut).toBe(5)
    expect(linesOfStep(shown!)[0]).toContain('5 earlier lines')
    expect(linesOfStep(shown!)).toHaveLength(MOST_LINES + 1)
  })

  it('takes the report as the whole of it when the run finishes', () => {
    const seen = hearing(hearing(beginning(NAMES), [began('gate'), printed('boom')]), [
      {
        kind: 'finished',
        report: report([word('plan', 'done'), word('gate', 'stopped', '`cargo test` exited 101'), word('commit', 'ahead')], false),
      },
    ])
    expect(seen.ended).toBe(true)
    expect(seen.current).toBeNull()
    expect(seen.report?.released).toBe(false)
    expect(step(seen, 'gate')?.status).toBe('stopped')
    expect(step(seen, 'gate')?.summary).toBe('`cargo test` exited 101')
    expect(step(seen, 'gate')?.lines).toEqual(['boom'])
    expect(step(seen, 'plan')?.status).toBe('done')
  })

  it('stops the step going on when the run could not go on at all', () => {
    const seen = hearing(hearing(beginning(NAMES), [began('gate')]), [{ kind: 'failed', reason: 'no shell' }])
    expect(seen.failed).toBe('no shell')
    expect(seen.ended).toBe(true)
    expect(seen.current).toBeNull()
    expect(step(seen, 'gate')?.status).toBe('stopped')
    expect(step(seen, 'gate')?.summary).toBe('no shell')
    expect(step(seen, 'commit')?.status).toBe('ahead')
  })

  it('leaves what it was given as it was', () => {
    const before = hearing(beginning(NAMES), [began('gate')])
    const lines = step(before, 'gate')?.lines
    hearing(before, [printed('later')])
    expect(step(before, 'gate')?.lines).toBe(lines)
    expect(lines).toEqual([])
  })
})

describe('a run found in its checkpoint', () => {
  it('shows what it passed, with nothing going on', () => {
    const seen = fromUnderWay({
      tag: 'v1.2.0',
      version: '1.2.0',
      resumable: true,
      steps: [word('plan', 'done', 'before the stop'), word('gate', 'ahead')],
      next: 'gate',
    })
    expect(seen.ended).toBe(true)
    expect(seen.current).toBeNull()
    expect(seen.steps.map((each) => each.status)).toEqual(['done', 'ahead'])
    expect(seen.steps[0]?.lines).toEqual([])
  })
})

describe('the step to show', () => {
  const after = (heard: Heard[]) => hearing(beginning(NAMES), heard)

  it('is the one going on', () => {
    expect(stepToShow(after([told(word('plan', 'stopped')), began('gate')]))).toBe('gate')
  })

  it('is the one that stopped, once nothing is going on', () => {
    expect(stepToShow(after([told(word('plan', 'stopped')), told(word('gate', 'done'))]))).toBe('plan')
  })

  it('is the last that did anything, once it went through', () => {
    expect(stepToShow(after([told(word('plan', 'done')), told(word('gate', 'skipped'))]))).toBe('gate')
  })

  it('is none before anything happened', () => {
    expect(stepToShow(beginning(NAMES))).toBeNull()
  })
})

const check = (name: string, status: Check['status']): Check => ({ name, status, summary: '', details: [] })

describe('the row on show', () => {
  it('starts on what stands in the way', () => {
    expect(firstToShow([check('a', 'pass'), check('b', 'warn'), check('c', 'fail')])).toEqual({ kind: 'check', name: 'c' })
    expect(firstToShow([check('a', 'pass'), check('b', 'warn')])).toEqual({ kind: 'check', name: 'b' })
    expect(firstToShow([check('a', 'pass'), check('b', 'skip')])).toEqual({ kind: 'check', name: 'a' })
    expect(firstToShow([])).toBeNull()
  })

  it('is what the reader chose, over everything', () => {
    const seen = hearing(beginning(NAMES), [began('gate')])
    expect(chosenOrShown({ kind: 'check', name: 'a' }, seen, [])).toEqual({ kind: 'check', name: 'a' })
  })

  it('follows the run until the reader chooses', () => {
    const seen = hearing(beginning(NAMES), [began('gate')])
    expect(chosenOrShown(null, seen, [check('a', 'fail')])).toEqual({ kind: 'step', name: 'gate' })
  })

  it('falls back on the plan where no run has done anything', () => {
    expect(chosenOrShown(null, beginning(NAMES), [check('a', 'fail')])).toEqual({ kind: 'check', name: 'a' })
    expect(chosenOrShown(null, null, [check('a', 'fail')])).toEqual({ kind: 'check', name: 'a' })
  })
})

describe('following the end of a log', () => {
  it('holds at the end, give or take a line', () => {
    expect(atTheEnd(600, 400, 1000)).toBe(true)
    expect(atTheEnd(580, 400, 1000)).toBe(true)
  })

  it('lets go once the reader scrolls away', () => {
    expect(atTheEnd(575, 400, 1000)).toBe(false)
    expect(atTheEnd(0, 400, 1000)).toBe(false)
  })
})

describe('what the buttons may do', () => {
  const state = (overrides: Partial<ReleaseState> = {}): ReleaseState => ({
    configured: true,
    steps: NAMES,
    broken: null,
    under_way: null,
    running: false,
    ...overrides,
  })
  const underWay = (resumable: boolean): ReleaseState['under_way'] => ({ tag: 'v1.2.0', version: '1.2.0', resumable, steps: [], next: resumable ? 'ci' : null })
  const plan = (ready: boolean, proposed = true): Plan =>
    ({ ready, proposed: proposed ? { version: '1.2.0', tag: 'v1.2.0', source: 'record', bump: 'minor' } : null }) as Plan

  it('runs a plan with no red in it', () => {
    expect(allowed(state(), plan(true), false)).toEqual({ run: true, resume: false })
  })

  it('does not run past a red check: red stops it', () => {
    expect(allowed(state(), plan(false), false)).toEqual({ run: false, resume: false })
  })

  it('does not run with nothing to release, or before the plan is here', () => {
    expect(allowed(state(), plan(true, false), false).run).toBe(false)
    expect(allowed(state(), null, false).run).toBe(false)
  })

  it('only carries on a run past its commit', () => {
    expect(allowed(state({ under_way: underWay(true) }), plan(true), false)).toEqual({ run: false, resume: true })
    expect(allowed(state({ under_way: underWay(true) }), plan(false), false).resume).toBe(true)
  })

  it('starts over from a run that stopped before its commit', () => {
    expect(allowed(state({ under_way: underWay(false) }), plan(true), false)).toEqual({ run: true, resume: false })
  })

  it('does nothing while a run is going, or with what cannot be read', () => {
    expect(allowed(state({ under_way: underWay(true) }), plan(true), true)).toEqual({ run: false, resume: false })
    expect(allowed(state({ broken: 'no' }), plan(true), false)).toEqual({ run: false, resume: false })
    expect(allowed(null, plan(true), false)).toEqual({ run: false, resume: false })
  })
})
