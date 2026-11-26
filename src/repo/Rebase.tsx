import { useEffect, useMemo, useState, type FormEvent, type KeyboardEvent, type PointerEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { GripVertical } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  ConfirmDialog,
  ConfirmDialogActions,
  ConfirmDialogClose,
  ConfirmDialogDescription,
  ConfirmDialogHeader,
  ConfirmDialogPopup,
  ConfirmDialogTitle,
} from '@/components/ui/confirm-dialog'
import { Dialog, DialogActions, DialogBody, DialogClose, DialogDescription, DialogHeader, DialogPopup, DialogTitle } from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { RowButton } from '@/components/ui/list-row'
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import { Textarea } from '@/components/ui/textarea'
import { api, failure } from '@/lib/api'
import type { AbsorbTarget } from '@/lib/generated/AbsorbTarget'
import type { Commit } from '@/lib/generated/Commit'
import type { FileChange } from '@/lib/generated/FileChange'
import type { Picked } from '@/lib/generated/Picked'
import type { RebasePlan } from '@/lib/generated/RebasePlan'
import { short } from '@/lib/names'
import { ACTS, changed, combined, leaves, moved, rowsOf, todoOf, whyNot, type Act, type Row } from '@/lib/rebase'
import { absorbInto, absorbLinesInto, dismiss, rebaseBy, rebaseOnto, squashTo, type Outcome } from '@/lib/remote'

/*
 * Rewriting the commits of a branch from the window: a rebase onto another
 * branch, a rebase by a plan - commits dragged to another place, reworded,
 * folded into the one above, dropped - a squash of the last commits into one,
 * and a change absorbed into a commit already made. What each means, and what
 * a plan may say, is furca-core's (ADR 0024); the dialogs show the commits as
 * the core read them and say what will become of them before it is done.
 */

/** A question about rewriting commits: which dialog is open. */
export type Rewriting =
  /** `name` is what git is told to land on and `label` how it is shown;
   * `into` the branch being rebased. */
  | { kind: 'rebase'; name: string; label: string; into: string }
  /** A plan for `commit` and every commit after it. */
  | { kind: 'plan'; commit: Commit; into: string }
  | { kind: 'squash'; commit: Commit; into: string }
  | { kind: 'absorb'; commit: Commit }

/** The dialog `asking` names, over the repository at `path`. */
export function RebaseDialogs({ path, asking, onDone }: { path: string; asking: Rewriting | null; onDone: () => void }) {
  const shut = (open: boolean) => {
    if (!open) onDone()
  }
  return (
    <>
      <RebaseOnto path={path} asking={asking?.kind === 'rebase' ? asking : null} onOpenChange={shut} />
      <Dialog open={asking?.kind === 'plan'} onOpenChange={shut}>
        <DialogPopup size="xl">
          {asking?.kind === 'plan' && <PlanForm key={asking.commit.id} path={path} asking={asking} onDone={onDone} />}
        </DialogPopup>
      </Dialog>
      <Dialog open={asking?.kind === 'squash'} onOpenChange={shut}>
        <DialogPopup size="lg">
          {asking?.kind === 'squash' && <SquashForm key={asking.commit.id} path={path} asking={asking} onDone={onDone} />}
        </DialogPopup>
      </Dialog>
      <AbsorbStaged path={path} asking={asking?.kind === 'absorb' ? asking : null} onOpenChange={shut} />
    </>
  )
}

/** The question before the current branch is replayed on another. */
function RebaseOnto({
  path,
  asking,
  onOpenChange,
}: {
  path: string
  asking: (Rewriting & { kind: 'rebase' }) | null
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const values = { name: asking?.label ?? '', into: asking?.into ?? '' }
  return (
    <ConfirmDialog open={asking !== null} onOpenChange={onOpenChange}>
      <ConfirmDialogPopup size="md">
        <ConfirmDialogHeader>
          <ConfirmDialogTitle className="break-words">{t('rebase.ontoTitle', values)}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{t('rebase.ontoBody', values)}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <ConfirmDialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</ConfirmDialogClose>
          <Button
            variant="primary"
            onClick={() => {
              if (asking === null) return
              onOpenChange(false)
              void rebaseOnto(path, asking.name)
            }}
          >
            {t('rebase.rebase')}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}

/** The question before what is staged goes into a commit already made. */
function AbsorbStaged({
  path,
  asking,
  onOpenChange,
}: {
  path: string
  asking: (Rewriting & { kind: 'absorb' }) | null
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const commit = asking?.commit
  const values = { at: commit === undefined ? '' : short(commit.id), subject: commit?.subject ?? '' }
  return (
    <ConfirmDialog open={asking !== null} onOpenChange={onOpenChange}>
      <ConfirmDialogPopup size="md">
        <ConfirmDialogHeader>
          <ConfirmDialogTitle className="break-words">{t('rebase.absorbTitle', values)}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{t('rebase.absorbBody', values)}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <ConfirmDialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</ConfirmDialogClose>
          <Button
            variant="primary"
            onClick={() => {
              if (commit === undefined) return
              onOpenChange(false)
              void absorbInto(path, commit.id)
            }}
          >
            {t('rebase.absorb')}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}

/** Something read for a dialog once it opens. */
type Read<T> = { state: 'reading' } | { state: 'read'; value: T } | { state: 'failed'; reason: string }

function useRead<T>(read: () => Promise<T>, key: string): Read<T> {
  const [held, setHeld] = useState<{ key: string; read: Read<T> }>({ key, read: { state: 'reading' } })
  useEffect(() => {
    let current = true
    read().then(
      (value) => {
        if (current) setHeld({ key, read: { state: 'read', value } })
      },
      (cause: unknown) => {
        if (current) setHeld({ key, read: { state: 'failed', reason: failure(cause) } })
      },
    )
    return () => {
      current = false
    }
    // `read` is made anew on every render; `key` says when it reads something else.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  return held.key === key ? held.read : { state: 'reading' }
}

/** How a dialog that runs git through the window's own runs stands: going,
 * and what git said if it refused. The plan stays on the screen then. */
function useStep(path: string, onDone: () => void) {
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const run = (step: Promise<Outcome>) => {
    setBusy(true)
    setSaid(null)
    void step.then((outcome) => {
      setBusy(false)
      if (outcome.ok) {
        onDone()
        return
      }
      // Said here, beside what was asked: not a second time behind the dialog.
      dismiss(path)
      setSaid(outcome.reason)
    })
  }
  return { busy, said, run }
}

function Refused({ title, said }: { title: string; said: string | null }) {
  if (said === null) return null
  return (
    <Alert tone="bad" title={title}>
      <span className="whitespace-pre-wrap">{said}</span>
    </Alert>
  )
}

/** What a dialog shows while its commits are read, or when they cannot be. */
function Waiting({ title, read }: { title: string; read: Read<unknown> }) {
  const { t } = useTranslation()
  return (
    <>
      <DialogHeader>
        <DialogTitle className="break-words">{title}</DialogTitle>
      </DialogHeader>
      <DialogBody>
        {read.state === 'failed' ? (
          <Alert tone="bad" title={t('rebase.readFailed')}>
            <span className="whitespace-pre-wrap">{read.reason}</span>
          </Alert>
        ) : (
          <div className="flex h-24 items-center justify-center">
            <Spinner label={t('rebase.reading')} />
          </div>
        )}
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
      </DialogActions>
    </>
  )
}

/** The words about commits the upstream has already, when there are any. */
function PushedWarning({ count, into }: { count: number; into: string }) {
  const { t } = useTranslation()
  if (count === 0) return null
  return <Alert tone="warn" title={t('rebase.pushedWarning', { count, into })} />
}

/** The plan for a commit and every commit after it, read and then arranged. */
function PlanForm({ path, asking, onDone }: { path: string; asking: Rewriting & { kind: 'plan' }; onDone: () => void }) {
  const { t } = useTranslation()
  const onto = asking.commit.parents[0] ?? null
  const read = useRead(() => api.rebasePlan(path, onto), `${path}\n${asking.commit.id}`)
  const title = t('rebase.planTitle', { into: asking.into, at: short(asking.commit.id) })
  if (read.state !== 'read') return <Waiting title={title} read={read} />
  return <PlanRows path={path} plan={read.value} onto={onto} title={title} into={asking.into} onDone={onDone} />
}

function PlanRows({
  path,
  plan,
  onto,
  title,
  into,
  onDone,
}: {
  path: string
  plan: RebasePlan
  onto: string | null
  title: string
  into: string
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [rows, setRows] = useState(() => rowsOf(plan))
  const original = useMemo(() => plan.steps.map((step) => step.commit.id), [plan])
  const [dragging, setDragging] = useState<string | null>(null)
  const { busy, said, run } = useStep(path, onDone)
  const notYet = whyNot(rows, original)
  const pushed = rows.filter((row) => row.pushed).length

  function submit(event: FormEvent) {
    event.preventDefault()
    if (notYet !== null) return
    run(rebaseBy(path, onto, todoOf(rows)))
  }

  // Dragged with pointer events, as the tabs are: the window takes files
  // dropped on it, which turns the webview's own drag and drop off.
  function startDrag(event: PointerEvent, id: string) {
    if (event.button !== 0) return
    event.preventDefault()
    setDragging(id)
    const onMove = (move: globalThis.PointerEvent) => {
      const over = document.elementFromPoint(move.clientX, move.clientY)?.closest<HTMLElement>('[data-plan-row]')
      const to = over?.dataset.planRow
      if (to === undefined || to === id) return
      setRows((now) => {
        const from = now.findIndex((row) => row.id === id)
        const at = now.findIndex((row) => row.id === to)
        return from === -1 || at === -1 ? now : moved(now, from, at)
      })
    }
    const stop = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', stop)
      window.removeEventListener('pointercancel', stop)
      setDragging(null)
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', stop)
    window.addEventListener('pointercancel', stop)
  }

  function keys(event: KeyboardEvent, at: number) {
    const to = event.key === 'ArrowUp' ? at - 1 : event.key === 'ArrowDown' ? at + 1 : null
    if (to === null) return
    event.preventDefault()
    setRows((now) => moved(now, at, to))
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DialogHeader>
        <DialogTitle className="break-words">{title}</DialogTitle>
        <DialogDescription>{t('rebase.planBody')}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <ol aria-label={t('rebase.listLabel')} className="flex flex-col gap-1">
          {rows.map((row, at) => (
            <PlanRow
              key={row.id}
              row={row}
              dragging={dragging === row.id}
              problem={notYet !== null && 'id' in notYet && notYet.id === row.id ? t(`rebase.not.${notYet.why}`, { at: short(row.id) }) : null}
              onGrab={(event) => startDrag(event, row.id)}
              onKeys={(event) => keys(event, at)}
              onChange={(part) => setRows((now) => changed(now, row.id, part))}
            />
          ))}
        </ol>
        <PushedWarning count={pushed} into={into} />
        <Refused title={t('rebase.refused')} said={said} />
      </DialogBody>
      <DialogActions>
        <span className="mr-auto min-w-0 truncate text-xs text-dim">
          {notYet?.why === 'unchanged' ? t('rebase.not.unchanged') : t('rebase.leaves', { count: leaves(rows), total: rows.length })}
        </span>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || notYet !== null}>
          {busy && <Spinner size="sm" label={t('reach.doing.rebase')} />}
          {t('rebase.start')}
        </Button>
      </DialogActions>
    </form>
  )
}

function PlanRow({
  row,
  dragging,
  problem,
  onGrab,
  onKeys,
  onChange,
}: {
  row: Row
  dragging: boolean
  problem: string | null
  onGrab: (event: PointerEvent) => void
  onKeys: (event: KeyboardEvent) => void
  onChange: (part: Partial<Pick<Row, 'act' | 'message'>>) => void
}) {
  const { t } = useTranslation()
  const at = short(row.id)
  const folded = row.act === 'squash' || row.act === 'fixup'
  return (
    <li
      data-plan-row={row.id}
      className={cn(
        'flex flex-col gap-1.5 rounded-md border border-line bg-raise px-1.5 py-1',
        dragging && 'border-accent opacity-70',
        problem !== null && 'border-bad',
      )}
    >
      <div className="flex items-center gap-2">
        <Button
          type="button"
          variant="icon"
          size="icon-xs"
          aria-label={t('rebase.move', { at })}
          title={t('rebase.move', { at })}
          className="shrink-0 cursor-grab touch-none"
          onPointerDown={onGrab}
          onKeyDown={onKeys}
        >
          <GripVertical aria-hidden />
        </Button>
        <Select value={row.act} onValueChange={(value) => onChange({ act: value as Act })}>
          <SelectTrigger size="sm" aria-label={t('rebase.actLabel', { at })} className="w-28 shrink-0">
            <SelectValue>{(value: unknown) => t(`rebase.act.${String(value)}`)}</SelectValue>
          </SelectTrigger>
          <SelectPopup>
            {ACTS.map((act) => (
              <SelectItem key={act} value={act}>
                {t(`rebase.act.${act}`)}
              </SelectItem>
            ))}
          </SelectPopup>
        </Select>
        <span className="shrink-0 font-mono text-2xs text-dim">{at}</span>
        <span
          title={row.original}
          className={cn('min-w-0 flex-1 truncate text-xs', row.act === 'drop' && 'text-faint line-through', folded && 'pl-3 text-dim')}
        >
          {row.subject}
        </span>
        {row.pushed && <Badge variant="warn">{t('rebase.pushed')}</Badge>}
      </div>
      {row.act === 'reword' && (
        <Textarea
          aria-label={t('rebase.message', { at })}
          value={row.message}
          rows={2}
          autoResize
          maxRows={8}
          spellCheck={false}
          onChange={(event) => onChange({ message: event.target.value })}
          className="font-mono text-xs"
        />
      )}
      {problem !== null && (
        <p role="alert" className="text-2xs text-bad">
          {problem}
        </p>
      )}
    </li>
  )
}

/** A commit and every commit after it made one, under a message that starts
 * as theirs one after another. */
function SquashForm({ path, asking, onDone }: { path: string; asking: Rewriting & { kind: 'squash' }; onDone: () => void }) {
  const { t } = useTranslation()
  const onto = asking.commit.parents[0] ?? null
  const read = useRead(() => api.rebasePlan(path, onto), `${path}\n${asking.commit.id}`)
  const title = t('rebase.squashTitle', { at: short(asking.commit.id) })
  if (read.state !== 'read') return <Waiting title={title} read={read} />
  return <SquashMessage path={path} plan={read.value} commit={asking.commit} into={asking.into} title={title} onDone={onDone} />
}

function SquashMessage({
  path,
  plan,
  commit,
  into,
  title,
  onDone,
}: {
  path: string
  plan: RebasePlan
  commit: Commit
  into: string
  title: string
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [message, setMessage] = useState(() => combined(plan.steps))
  const { busy, said, run } = useStep(path, onDone)
  const pushed = plan.steps.filter((step) => step.pushed).length

  function submit(event: FormEvent) {
    event.preventDefault()
    run(squashTo(path, commit.id, message))
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DialogHeader>
        <DialogTitle className="break-words">{title}</DialogTitle>
        <DialogDescription>{t('rebase.squashBody', { count: plan.steps.length, at: short(commit.id), into })}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <Field label={t('rebase.squashMessage')}>
          <Textarea
            autoFocus
            rows={6}
            autoResize
            maxRows={16}
            spellCheck={false}
            value={message}
            onChange={(event) => setMessage(event.target.value)}
            className="font-mono text-xs"
          />
        </Field>
        <PushedWarning count={pushed} into={into} />
        <Refused title={t('rebase.squashRefused')} said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || message.trim() === '' || plan.steps.length < 2}>
          {busy && <Spinner size="sm" label={t('reach.doing.squash')} />}
          {t('rebase.squash')}
        </Button>
      </DialogActions>
    </form>
  )
}

/** Lines of a file that is not staged, put into a commit chosen from the
 * line `HEAD` came by: open while `picks` names them. */
export function AbsorbLines({
  path,
  file,
  picks,
  onOpenChange,
}: {
  path: string
  file: FileChange | null
  picks: Picked[] | null
  onOpenChange: (open: boolean) => void
}) {
  const open = picks !== null && file !== null
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup size="lg">
        {open && <AbsorbForm path={path} file={file} picks={picks} onDone={() => onOpenChange(false)} />}
      </DialogPopup>
    </Dialog>
  )
}

function AbsorbForm({ path, file, picks, onDone }: { path: string; file: FileChange; picks: Picked[]; onDone: () => void }) {
  const { t } = useTranslation()
  const read = useRead(() => api.absorbTargets(path, file.path), `${path}\n${file.path}`)
  const title = t('rebase.absorbLinesTitle')
  if (read.state !== 'read') return <Waiting title={title} read={read} />
  return <AbsorbTargets path={path} file={file} picks={picks} targets={read.value} title={title} onDone={onDone} />
}

function AbsorbTargets({
  path,
  file,
  picks,
  targets,
  title,
  onDone,
}: {
  path: string
  file: FileChange
  picks: Picked[]
  targets: AbsorbTarget[]
  title: string
  onDone: () => void
}) {
  const { t } = useTranslation()
  // The newest commit that changed this file is the likeliest home for it.
  const [chosen, setChosen] = useState<string | null>(() => targets.find((target) => target.touches)?.commit.id ?? null)
  const { busy, said, run } = useStep(path, onDone)
  const lines = picks.reduce((sum, each) => sum + each.removed.length + each.added.length, 0)
  const target = targets.find((each) => each.commit.id === chosen)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (chosen === null) return
    run(absorbLinesInto(path, file, picks, chosen))
  }

  let list: ReactNode
  if (targets.length === 0) {
    list = <p className="text-xs text-dim">{t('rebase.noTargets')}</p>
  } else {
    list = (
      <div role="listbox" aria-label={t('rebase.targetsLabel')} className="flex max-h-72 flex-col gap-0.5 overflow-y-auto">
        {targets.map((each) => (
          <RowButton
            key={each.commit.id}
            role="option"
            aria-selected={each.commit.id === chosen}
            selected={each.commit.id === chosen}
            start={<span className="font-mono text-2xs text-dim">{short(each.commit.id)}</span>}
            end={
              <>
                {each.touches && <Badge variant="soft">{t('rebase.touches')}</Badge>}
                {each.pushed && <Badge variant="warn">{t('rebase.pushed')}</Badge>}
              </>
            }
            onClick={() => setChosen(each.commit.id)}
          >
            {each.commit.subject}
          </RowButton>
        ))}
      </div>
    )
  }

  return (
    <form onSubmit={submit} className="flex min-h-0 flex-1 flex-col">
      <DialogHeader>
        <DialogTitle>{title}</DialogTitle>
        <DialogDescription>{t('rebase.absorbLinesBody', { count: lines, path: file.path })}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        {list}
        <PushedWarning count={target?.pushed === true ? 1 : 0} into={t('rebase.theBranch')} />
        <Refused title={t('rebase.absorbRefused')} said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || chosen === null}>
          {busy && <Spinner size="sm" label={t('reach.doing.absorb')} />}
          {t('rebase.absorb')}
        </Button>
      </DialogActions>
    </form>
  )
}
