import { useSyncExternalStore } from 'react'
import { listen } from '@tauri-apps/api/event'
import { api, failure } from './api'
import type { Check } from './generated/Check'
import type { Heard } from './generated/Heard'
import type { Plan } from './generated/Plan'
import type { ReleaseHeard } from './generated/ReleaseHeard'
import type { ReleaseState } from './generated/ReleaseState'
import type { Report } from './generated/Report'
import type { RunStep } from './generated/RunStep'
import type { StepStatus } from './generated/StepStatus'
import type { UnderWay } from './generated/UnderWay'

/*
 * A repository's release as the window knows it: the plan it asked for, and the
 * run it hears (`src-tauri/src/release.rs`). A run outlives the panel and the tab
 * in front, so it is held here for as long as the window is.
 */

/** A step, as a run has told it so far. */
export interface SeenStep {
  name: string
  /** `running` between the step beginning and ending. */
  status: StepStatus | 'running'
  summary: string
  details: string[]
  /** What the step said and what the programs it ran printed, in order. */
  lines: string[]
  /** How many of its first lines were let go. */
  cut: number
}

/** A run, as far as the window has heard it. */
export interface Seen {
  steps: SeenStep[]
  /** The step going on now. */
  current: string | null
  /** The run has ended. */
  ended: boolean
  report: Report | null
  /** Why it could not go on at all. */
  failed: string | null
}

/** Lines kept of each step, the newest. A gate prints tens of thousands, and
 * its last screenful is what says why it stopped. */
export const MOST_LINES = 20_000

const stepsNamed = (names: readonly string[]): SeenStep[] =>
  names.map((name) => ({ name, status: 'ahead', summary: '', details: [], lines: [], cut: 0 }))

/** A run about to begin, its steps as the engine names them. */
export function beginning(steps: readonly string[]): Seen {
  return { steps: stepsNamed(steps), current: null, ended: false, report: null, failed: null }
}

/** A run found in its checkpoint: what it passed, and where it stopped. */
export function fromUnderWay(underWay: UnderWay): Seen {
  return {
    steps: underWay.steps.map((step) => ({ ...step, lines: [], cut: 0 })),
    current: null,
    ended: true,
    report: null,
    failed: null,
  }
}

function withLines(step: SeenStep, added: readonly string[]): SeenStep {
  const lines = step.lines.concat(added)
  const over = Math.max(0, lines.length - MOST_LINES)
  return { ...step, lines: over === 0 ? lines : lines.slice(over), cut: step.cut + over }
}

/** Marks a step from the engine's word for how it ended, keeping its lines. */
const ended = (step: SeenStep, word: RunStep): SeenStep => ({ ...step, status: word.status, summary: word.summary, details: word.details })

/** What a run becomes on hearing `heard`, a batch in the order it happened. */
export function hearing(seen: Seen, heard: readonly Heard[]): Seen {
  let steps = seen.steps
  let { current, report, failed } = seen
  let finished = seen.ended
  const change = (name: string, update: (step: SeenStep) => SeenStep) => {
    steps = steps.map((step) => (step.name === name ? update(step) : step))
  }
  for (const each of heard) {
    if (each.kind === 'printed') {
      // Printed before any step began belongs to none of them, and there is
      // nowhere to show it; the engine begins `plan` before it runs anything.
      if (current !== null) change(current, (step) => withLines(step, [each.line]))
    } else if (each.kind === 'finished') {
      report = each.report
      finished = true
      current = null
      // The report is the whole of it: a step told nothing still has its word
      // here, and a step left going has ended.
      const words = new Map(each.report.steps.map((step) => [step.name, step]))
      steps = steps.map((step) => {
        const word = words.get(step.name)
        return word === undefined ? step : ended(step, word)
      })
    } else if (each.kind === 'failed') {
      failed = each.reason
      finished = true
      current = null
      steps = steps.map((step) => (step.status === 'running' ? { ...step, status: 'stopped', summary: each.reason } : step))
    } else {
      const { told } = each
      if (told.kind === 'began') {
        current = told.step
        change(told.step, (step) => ({ ...step, status: 'running' }))
      } else if (told.kind === 'said') {
        change(told.step, (step) => withLines(step, [told.line]))
      } else {
        change(told.step.name, (step) => ended(step, told.step))
        if (current === told.step.name) current = null
      }
    }
  }
  return { steps, current, ended: finished, report, failed }
}

/** The step to show when the reader has not chosen one: the one going on, then
 * the one that stopped, then the last that did anything. */
export function stepToShow(seen: Seen): string | null {
  if (seen.current !== null) return seen.current
  const stopped = seen.steps.find((step) => step.status === 'stopped')
  if (stopped !== undefined) return stopped.name
  return seen.steps.findLast((step) => step.status !== 'ahead')?.name ?? null
}

/** A row of the panel: a check of the plan, or a step of the run. */
export interface Chosen {
  kind: 'check' | 'step'
  name: string
}

/** The row to show first before a run: the first check that fails, then the
 * first worth reading, then the first of all. */
export function firstToShow(checks: readonly Check[]): Chosen | null {
  const check = checks.find((each) => each.status === 'fail') ?? checks.find((each) => each.status === 'warn') ?? checks[0]
  return check === undefined ? null : { kind: 'check', name: check.name }
}

/** The row on show: the one the reader chose, then the step a run is on or
 * stopped at, then what the plan has to say. */
export function chosenOrShown(picked: Chosen | null, seen: Seen | null, checks: readonly Check[]): Chosen | null {
  if (picked !== null) return picked
  const step = seen === null ? null : stepToShow(seen)
  return step === null ? firstToShow(checks) : { kind: 'step', name: step }
}

/** What a step's log shows, saying so when its first lines were let go. */
export function linesOfStep(step: SeenStep): readonly string[] {
  return step.cut === 0 ? step.lines : [`... ${String(step.cut)} earlier lines are not kept`, ...step.lines]
}

/** Whether a box scrolled to `scrollTop` shows the end of what is in it, give or
 * take a line: a reader this close means to follow it. */
export function atTheEnd(scrollTop: number, viewportHeight: number, scrollHeight: number, slack = 24): boolean {
  return scrollTop + viewportHeight >= scrollHeight - slack
}

/** What the panel's buttons may do now, and the engine would refuse otherwise:
 * a red check stops a run before it starts, a run past its commit can only be
 * carried on, and one that has nothing to carry on starts over. */
export function allowed(state: ReleaseState | null, plan: Plan | null, running: boolean): { run: boolean; resume: boolean } {
  if (state === null || running || state.broken !== null) return { run: false, resume: false }
  const resumable = state.under_way?.resumable === true
  return { run: !resumable && plan !== null && plan.ready && plan.proposed !== null, resume: resumable }
}

/** A read that may be under way. */
export type PlanRead = { state: 'none' } | { state: 'reading'; last: Plan | null } | { state: 'read'; plan: Plan } | { state: 'failed'; reason: string }

/** One repository's release, as the panel draws it. */
export interface Held {
  state: ReleaseState | null
  plan: PlanRead
  /** The run heard in this window, or the one found in the checkpoint. */
  seen: Seen | null
  running: boolean
}

const NOTHING: Held = { state: null, plan: { state: 'none' }, seen: null, running: false }

const held = new Map<string, Held>()
const listeners = new Set<() => void>()

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

function change(path: string, update: (now: Held) => Held) {
  held.set(path, update(held.get(path) ?? NOTHING))
  for (const listener of listeners) listener()
}

let listening: Promise<unknown> | null = null

/** Starts listening, for as long as the window is there. */
export function listenForReleases(): Promise<unknown> {
  listening ??= listen<ReleaseHeard>('release', ({ payload }) => {
    const finishes = payload.heard.some((each) => each.kind === 'finished' || each.kind === 'failed')
    change(payload.path, (now) => ({
      ...now,
      seen: hearing(now.seen ?? beginning([]), payload.heard),
      running: !finishes && now.running,
    }))
    if (finishes) {
      void readState(payload.path)
      void readPlan(payload.path)
    }
  })
  return listening
}

/** Asks what the panel of `path` starts from; a run found in its checkpoint is
 * shown as it stopped, and a run heard here stays as it was heard. */
export async function readState(path: string): Promise<void> {
  try {
    const state = await api.releaseState(path)
    change(path, (now) => ({
      ...now,
      state,
      running: state.running || now.running,
      seen: now.seen ?? (state.under_way === null ? null : fromUnderWay(state.under_way)),
    }))
  } catch (cause) {
    change(path, (now) => ({
      ...now,
      state: { configured: true, steps: now.state?.steps ?? [], broken: failure(cause), under_way: null, running: false },
    }))
  }
}

/** Asks for the plan again, keeping the last on the screen until it answers. */
export async function readPlan(path: string): Promise<void> {
  change(path, (now) => ({
    ...now,
    plan: { state: 'reading', last: now.plan.state === 'read' ? now.plan.plan : now.plan.state === 'reading' ? now.plan.last : null },
  }))
  try {
    const plan = await api.releasePlan(path)
    change(path, (now) => ({ ...now, plan: { state: 'read', plan } }))
  } catch (cause) {
    change(path, (now) => ({ ...now, plan: { state: 'failed', reason: failure(cause) } }))
  }
}

/** Starts a run, or carries on the one that stopped. */
export async function startRun(path: string, resume: boolean, steps: readonly string[]): Promise<void> {
  await listenForReleases()
  change(path, (now) => ({ ...now, running: true, seen: beginning(steps) }))
  try {
    await api.releaseRun(path, resume)
  } catch (cause) {
    change(path, (now) => ({
      ...now,
      running: false,
      seen: hearing(now.seen ?? beginning(steps), [{ kind: 'failed', reason: failure(cause) }]),
    }))
  }
}

/** The release of the repository at `path`. */
export function useRelease(path: string): Held {
  return useSyncExternalStore(subscribe, () => held.get(path) ?? NOTHING)
}
