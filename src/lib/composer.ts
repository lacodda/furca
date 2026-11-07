import type { Parts } from './generated/Parts'
import type { Warning } from './generated/Warning'

/*
 * The sums behind the composer, with no React in them. What a message means is
 * not decided here: `furca_release::message` reads it and the repository's own
 * hook judges it (ADR 0019). This only puts the fields together and takes what
 * those two say and arranges it for the panel.
 */

/** What the form holds. An empty `kind` is a subject out of the convention,
 * which is all description: a repository that does not write commits this way
 * is not made to. */
export interface Draft {
  kind: string
  scope: string
  breaking: boolean
  description: string
  body: string
}

export const EMPTY: Draft = { kind: 'feat', scope: '', breaking: false, description: '', body: '' }

/** The message the form says, as git will be given it. */
export function compose({ kind, scope, breaking, description, body }: Draft): string {
  const text = description.trim()
  const name = scope.trim()
  const subject = kind === '' ? text : `${kind}${name === '' ? '' : `(${name})`}${breaking ? '!' : ''}: ${text}`
  const rest = body.trim()
  return rest === '' ? subject : `${subject}\n\n${rest}`
}

/** A message the core took apart, as the form shows it. */
export function fromParts(parts: Parts): Draft {
  return {
    kind: parts.kind ?? '',
    scope: parts.scope ?? '',
    breaking: parts.breaking,
    description: parts.description,
    body: parts.body,
  }
}

/** The form once its commit is made: the same kind of work often goes on. */
export function afterCommit(draft: Draft): Draft {
  return { ...EMPTY, kind: draft.kind }
}

/** Why the button cannot be pressed, as a key under `composer.not`; `null` when
 * it can. Only what git would refuse in any case: everything else is a word of
 * advice, and what a hook refuses is the hook's to say when it runs. */
export type NotYet = 'conflicted' | 'nothing-staged' | 'no-description' | 'busy'

export function whyNot(draft: Draft, staged: number, conflicted: number, amend: boolean, busy: boolean): NotYet | null {
  if (busy) return 'busy'
  if (conflicted > 0) return 'conflicted'
  if (staged === 0 && !amend) return 'nothing-staged'
  if (draft.description.trim() === '') return 'no-description'
  return null
}

/** Whether a draft may fill the form: never over what a person has typed. */
export function canDraft(draft: Draft): boolean {
  return draft.description.trim() === '' && draft.body.trim() === ''
}

/** The doctor's words about one kind of thing, with the files they are about. */
export interface WarningGroup {
  kind: Warning['kind']
  paths: string[]
}

/** The doctor's words by kind, in the order the panel says them. */
export function grouped(warnings: readonly Warning[]): WarningGroup[] {
  const kinds: Warning['kind'][] = ['assistant', 'large', 'endings']
  return kinds.flatMap((kind) => {
    const paths = warnings.filter((warning) => warning.kind === kind).map((warning) => warning.path)
    return paths.length === 0 ? [] : [{ kind, paths }]
  })
}

/** A short id as the panel says it. */
export const shortId = (id: string): string => id.slice(0, 7)
