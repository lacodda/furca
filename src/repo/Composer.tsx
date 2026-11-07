import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Sparkles, TriangleAlert } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { keysOf } from '@/components/ui/shortcut'
import { Spinner } from '@/components/ui/spinner'
import { Textarea } from '@/components/ui/textarea'
import { api, failure } from '@/lib/api'
import { afterCommit, canDraft, compose, EMPTY, fromParts, grouped, shortId, whyNot, type Draft } from '@/lib/composer'
import type { Finding } from '@/lib/generated/Finding'
import type { MessageCheck } from '@/lib/generated/MessageCheck'
import type { Warning } from '@/lib/generated/Warning'

/** How long typing pauses before the message is read and the hook asked. */
const PAUSE = 300

/** The value the type's list has for a subject out of the convention. */
const NO_TYPE = 'none'

/** Files a warning names before it says how many more. */
const NAMED = 2

/** What each repository's form held, kept while the window is: going to the
 * history to look something up does not lose a message half written. */
const kept = new Map<string, { draft: Draft; amend: boolean; before: Draft | null }>()

/** The types of the convention and whether Claude Code is here, asked once. */
let types: Promise<string[]> | null = null
let drafting: Promise<boolean> | null = null

export interface ComposerProps {
  path: string
  /** How many files are staged. */
  staged: number
  /** How many files a merge left unresolved. */
  conflicted: number
  /** What the doctor says of what is staged. */
  warnings: readonly Warning[]
}

/**
 * The commit to be made: its type, scope, description and body, what stands
 * against it, and the button. Nothing here decides what a message may say. The
 * core reads it as the release engine will, and the repository's `commit-msg`
 * hook, the one `git commit` runs, says what it has to say as you type.
 */
export function Composer({ path, staged, conflicted, warnings }: ComposerProps) {
  const { t } = useTranslation()
  const held = kept.get(path)
  const [draft, setDraft] = useState<Draft>(held?.draft ?? EMPTY)
  const [amend, setAmend] = useState(held?.amend ?? false)
  // The form as it was before an amend filled it, for when the amend is called off.
  const before = useRef<Draft | null>(held?.before ?? null)
  const [pushed, setPushed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [failed, setFailed] = useState<string | null>(null)
  const [made, setMade] = useState<string | null>(null)
  const [check, setCheck] = useState<MessageCheck | null>(null)
  const [kinds, setKinds] = useState<readonly string[]>([])
  const [canAsk, setCanAsk] = useState(false)
  const [asking, setAsking] = useState(false)

  useEffect(() => {
    kept.set(path, { draft, amend, before: before.current })
  })

  useEffect(() => {
    let current = true
    types ??= api.commitTypes()
    drafting ??= api.draftAvailable()
    void types.then((list) => {
      if (current) setKinds(list)
    })
    void drafting.then((here) => {
      if (current) setCanAsk(here)
    })
    return () => {
      current = false
    }
  }, [])

  const message = compose(draft)
  const empty = draft.description.trim() === ''
  useEffect(() => {
    if (empty) return
    let current = true
    const timer = setTimeout(() => {
      api.checkMessage(path, message).then(
        (read) => {
          if (current) setCheck(read)
        },
        // Not being able to ask is no reason to say anything: the commit asks again.
        () => {
          if (current) setCheck(null)
        },
      )
    }, PAUSE)
    return () => {
      current = false
      clearTimeout(timer)
    }
  }, [path, message, empty])

  const change = (part: Partial<Draft>) => {
    setDraft((now) => ({ ...now, ...part }))
    setMade(null)
    setFailed(null)
  }

  const toggleAmend = (next: boolean) => {
    setFailed(null)
    setMade(null)
    setAmend(next)
    if (!next) {
      setPushed(false)
      setDraft(before.current ?? EMPTY)
      before.current = null
      return
    }
    before.current = draft
    api.lastCommit(path).then(
      (last) => {
        setDraft(fromParts(last.parts))
        setPushed(last.pushed)
      },
      (cause: unknown) => {
        setFailed(failure(cause))
        setAmend(false)
      },
    )
  }

  const notYet = whyNot(draft, staged, conflicted, amend, busy)

  const commit = () => {
    if (notYet !== null) return
    setBusy(true)
    setFailed(null)
    setMade(null)
    api
      .commit(path, message, amend)
      .then(
        (id) => {
          setMade(id)
          setDraft(afterCommit(amend && before.current !== null ? before.current : draft))
          before.current = null
          setAmend(false)
          setPushed(false)
          setCheck(null)
        },
        (cause: unknown) => {
          setFailed(failure(cause))
        },
      )
      .finally(() => {
        setBusy(false)
      })
  }

  const askClaude = () => {
    setAsking(true)
    setFailed(null)
    api
      .draftMessage(path)
      .then(
        (parts) => {
          setDraft(fromParts(parts))
        },
        (cause: unknown) => {
          setFailed(failure(cause))
        },
      )
      .finally(() => {
        setAsking(false)
      })
  }

  const keys = (event: KeyboardEvent) => {
    if (keysOf(event.nativeEvent) === 'Mod+Enter') {
      event.preventDefault()
      commit()
    }
  }

  const said = empty ? null : check
  const groups = grouped(warnings)
  // A commit the hook refused says what the hook said a moment ago: once is enough.
  const refused = said?.refused != null && !(failed?.includes(said.refused) ?? false) ? said.refused : null
  const anything = refused !== null || failed !== null || groups.length > 0 || pushed || (said?.findings.length ?? 0) > 0
  const kindValue = draft.kind === '' ? NO_TYPE : draft.kind
  const listed = kinds.includes(draft.kind) || draft.kind === '' ? kinds : [...kinds, draft.kind]

  return (
    <form
      aria-label={t('composer.label')}
      onKeyDown={keys}
      onSubmit={(event) => {
        event.preventDefault()
        commit()
      }}
      className="flex shrink-0 flex-col gap-1.5 border-t border-line bg-raise p-2 text-xs"
    >
      <div className="flex items-center gap-1.5">
        <Select value={kindValue} onValueChange={(value) => change({ kind: value === NO_TYPE || value === null ? '' : String(value) })}>
          <SelectTrigger size="sm" aria-label={t('composer.type')} className="w-24 shrink-0">
            <SelectValue>{(value: unknown) => (value === NO_TYPE ? t('composer.noType') : String(value))}</SelectValue>
          </SelectTrigger>
          <SelectPopup>
            {listed.map((kind) => (
              <SelectItem key={kind} value={kind}>
                {kind}
              </SelectItem>
            ))}
            <SelectItem value={NO_TYPE}>{t('composer.noType')}</SelectItem>
          </SelectPopup>
        </Select>
        <Input
          aria-label={t('composer.scope')}
          placeholder={t('composer.scope')}
          value={draft.scope}
          disabled={draft.kind === ''}
          onChange={(event) => change({ scope: event.target.value })}
          className="h-control-sm min-w-0 flex-1 text-xs"
        />
        <Checkbox checked={draft.breaking} disabled={draft.kind === ''} onCheckedChange={(breaking) => change({ breaking })}>
          <span title={t('composer.breakingHint')}>{t('composer.breaking')}</span>
        </Checkbox>
      </div>
      <Input
        aria-label={t('composer.description')}
        placeholder={t('composer.description')}
        value={draft.description}
        onChange={(event) => change({ description: event.target.value })}
        className="h-control-sm text-xs"
      />
      <Textarea
        aria-label={t('composer.body')}
        placeholder={t('composer.body')}
        value={draft.body}
        rows={2}
        autoResize
        maxRows={8}
        onChange={(event) => change({ body: event.target.value })}
        className="text-xs"
      />

      {/* However much there is to say, the button stays where it is. */}
      {anything && (
        <ul aria-label={t('composer.doctorLabel')} className="flex max-h-32 min-h-0 flex-col gap-1 overflow-y-auto text-2xs text-warn">
          {refused !== null && <Said bad title={t('composer.hookRefuses')} text={refused} />}
          {failed !== null && <Said bad title={t('composer.failed')} text={failed} />}
          {said?.findings.map((finding) => <Said key={finding.kind} text={findingText(t, finding)} />)}
          {pushed && <Said text={t('composer.pushed')} />}
          {groups.map((group) => (
            <Said
              key={group.kind}
              hint={group.paths.join('\n')}
              text={`${t(`composer.doctor.${group.kind}`, { count: group.paths.length })}: ${group.paths.slice(0, NAMED).join(', ')}${
                group.paths.length > NAMED ? ` ${t('composer.andMore', { count: group.paths.length - NAMED })}` : ''
              }`}
            />
          ))}
        </ul>
      )}

      {made !== null && (
        <p role="status" className="truncate text-2xs text-dim">
          {t('composer.made', { id: shortId(made) })}
        </p>
      )}

      <div className="flex items-center gap-2">
        <Checkbox checked={amend} disabled={busy} onCheckedChange={toggleAmend}>
          {t('composer.amend')}
        </Checkbox>
        {canAsk && (
          <Button
            type="button"
            variant="icon"
            size="xs"
            disabled={asking || staged === 0 || !canDraft(draft)}
            disabledReason={asking ? t('composer.asking') : staged === 0 ? t('composer.not.nothing-staged') : !canDraft(draft) ? t('composer.draftOver') : undefined}
            title={t('composer.draftHint')}
            onClick={askClaude}
          >
            {asking ? <Spinner size="sm" label={t('composer.asking')} /> : <Sparkles aria-hidden />}
            {t('composer.draft')}
          </Button>
        )}
        <Button
          type="submit"
          variant="primary"
          size="sm"
          className="ml-auto shrink-0"
          disabled={notYet !== null}
          disabledReason={notYet === null ? undefined : t(`composer.not.${notYet}`)}
        >
          {busy && <Spinner size="sm" label={t('composer.not.busy')} />}
          {amend ? t('composer.commitAmend') : staged === 0 ? t('composer.commit') : t('composer.commitFiles', { count: staged })}
        </Button>
      </div>
    </form>
  )
}

/** One thing the panel has to say, in the words of whoever said it. */
function Said({ text, title, hint, bad = false }: { text: string; title?: string; hint?: string; bad?: boolean }) {
  return (
    <li className={cn('flex items-start gap-1', bad && 'text-bad')} title={hint}>
      <TriangleAlert aria-hidden className="mt-px size-3 shrink-0" />
      <span className="min-w-0 break-words whitespace-pre-wrap">
        {title !== undefined && <span className="block font-medium">{title}</span>}
        {text}
      </span>
    </li>
  )
}

function findingText(t: ReturnType<typeof useTranslation>['t'], finding: Finding): string {
  switch (finding.kind) {
    case 'empty':
      return t('composer.finding.empty')
    case 'not-conventional':
      return t('composer.finding.notConventional')
    case 'unknown-type':
      return t('composer.finding.unknownType', { name: finding.name })
    case 'no-blank-line':
      return t('composer.finding.noBlankLine')
    case 'long-subject':
      return t('composer.finding.longSubject', { length: finding.length, most: finding.most })
    case 'final-period':
      return t('composer.finding.finalPeriod')
    case 'not-english':
      return t('composer.finding.notEnglish')
  }
}
