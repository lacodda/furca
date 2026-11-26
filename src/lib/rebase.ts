import type { RebasePlan } from './generated/RebasePlan'
import type { RebaseStep } from './generated/RebaseStep'
import type { RebaseTodo } from './generated/RebaseTodo'

/*
 * The sums behind the rebase dialog, with no React in them. What a plan may
 * say is the core's to hold it to (`Git::rebase_interactive`, ADR 0024): every
 * commit named once, nothing folded into nothing. This arranges the rows a
 * person moves about, says the same two things before the button is pressed,
 * and writes the list the core takes.
 */

/** What becomes of a commit, in git's own words. */
export type Act = 'pick' | 'reword' | 'squash' | 'fixup' | 'drop'

export const ACTS: readonly Act[] = ['pick', 'reword', 'squash', 'fixup', 'drop']

/** One commit of the plan as the dialog holds it. */
export interface Row {
  id: string
  subject: string
  /** Its message as it was written. */
  original: string
  pushed: boolean
  act: Act
  /** The message a reword gives it; the original until it is edited. */
  message: string
}

/** The plan's commits as rows, each picked as it is, oldest first. */
export function rowsOf(plan: RebasePlan): Row[] {
  return plan.steps.map((step) => ({
    id: step.commit.id,
    subject: step.commit.subject,
    original: step.message,
    pushed: step.pushed,
    act: 'pick',
    message: step.message,
  }))
}

/** `rows` with the row at `from` put where the row at `to` is. */
export function moved(rows: readonly Row[], from: number, to: number): Row[] {
  const out = [...rows]
  if (from === to || from < 0 || to < 0 || from >= out.length || to >= out.length) return out
  const [row] = out.splice(from, 1)
  if (row !== undefined) out.splice(to, 0, row)
  return out
}

/** `rows` with the row `id` given `part`. */
export function changed(rows: readonly Row[], id: string, part: Partial<Pick<Row, 'act' | 'message'>>): Row[] {
  return rows.map((row) => (row.id === id ? { ...row, ...part } : row))
}

/** Whether a row is replayed under a message of its own choosing. */
function reworded(row: Row): boolean {
  return row.act === 'reword' && row.message.trim() !== row.original.trim()
}

/** Why the plan cannot be run, and the commit it is about; `null` when it
 * can. `unchanged` is a plan that would replay every commit as it is. */
export type NotYet =
  | { why: 'nothing-before'; id: string }
  | { why: 'no-message'; id: string }
  | { why: 'unchanged' }

export function whyNot(rows: readonly Row[], original: readonly string[]): NotYet | null {
  let any = false
  for (const row of rows) {
    if ((row.act === 'squash' || row.act === 'fixup') && !any) return { why: 'nothing-before', id: row.id }
    if (row.act === 'reword' && row.message.trim() === '') return { why: 'no-message', id: row.id }
    if (row.act === 'pick' || row.act === 'reword') any = true
  }
  const same = rows.every((row, at) => row.id === original[at] && (row.act === 'pick' || (row.act === 'reword' && !reworded(row))))
  return same ? { why: 'unchanged' } : null
}

/** The list the core takes: every row once, in order. A reword that left the
 * message as it was is a pick. */
export function todoOf(rows: readonly Row[]): RebaseTodo[] {
  return rows.map((row): RebaseTodo => {
    if (row.act === 'reword') {
      return reworded(row) ? { commit: row.id, action: 'reword', message: row.message.trim() } : { commit: row.id, action: 'pick' }
    }
    return { commit: row.id, action: row.act }
  })
}

/** How many commits the plan leaves, once it has folded and dropped. */
export function leaves(rows: readonly Row[]): number {
  return rows.filter((row) => row.act === 'pick' || row.act === 'reword').length
}

/** The messages of `steps` one after another, oldest first: what a squash
 * starts its one message from. */
export function combined(steps: readonly RebaseStep[]): string {
  return steps.map((step) => step.message.trim()).join('\n\n')
}
