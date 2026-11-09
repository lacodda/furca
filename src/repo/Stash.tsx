import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Archive } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogClose,
  DialogDescription,
  DialogHeader,
  DialogPopup,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { api, failure } from '@/lib/api'
import type { Stash } from '@/lib/generated/Stash'
import type { StashAction } from '@/lib/generated/StashAction'
import { useSidebar } from '@/lib/repos'
import { counted, useWorkingCopy } from '@/lib/working'
import { ConfirmDiscard } from './ConfirmDiscard'

/*
 * The stash from the window: putting every change away, and getting an entry
 * back. An entry is read like any commit - choosing it in the left column
 * opens it under the graph, untracked files and all - and what can be done to
 * it is said over its files there.
 */

/**
 * The button that puts every change away - staged, not staged and untracked -
 * with a message to know it by, and leaves the working tree as `HEAD` has it.
 * It asks for the message first, and says how much goes.
 */
export function StashButton({ path, labelClass }: { path: string; labelClass?: string }) {
  const { t } = useTranslation()
  const copy = useWorkingCopy(path)
  const [open, setOpen] = useState(false)
  const [message, setMessage] = useState('')
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const total = copy.state === 'read' ? counted(copy.status, copy.counts) : null
  const nothing = total !== null && total.total === 0

  function submit(event: FormEvent) {
    event.preventDefault()
    setBusy(true)
    setSaid(null)
    api.stashPush(path, message.trim() === '' ? null : message.trim()).then(
      (made) => {
        setBusy(false)
        if (made === null) {
          setSaid(t('stash.nothing'))
          return
        }
        setMessage('')
        setOpen(false)
      },
      (cause: unknown) => {
        setBusy(false)
        setSaid(failure(cause))
      },
    )
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (next) setSaid(null)
      }}
    >
      <Button
        variant="ghost"
        size="sm"
        className="shrink-0"
        disabled={nothing}
        title={nothing ? t('stash.nothing') : t('stash.button')}
        onClick={() => setOpen(true)}
      >
        <Archive aria-hidden />
        <span className={labelClass}>{t('stash.stash')}</span>
      </Button>
      <DialogPopup size="sm">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{t('stash.title')}</DialogTitle>
            <DialogDescription>
              {total === null
                ? t('stash.bodyUnknown')
                : t('stash.body', {
                    changed: t('stash.changed', { count: total.changed }),
                    untracked: t('stash.untracked', { count: total.untracked }),
                  })}
            </DialogDescription>
          </DialogHeader>
          <DialogBody className="flex flex-col gap-3">
            <Field label={t('stash.message')} help={t('stash.messageHelp')}>
              <Input autoFocus value={message} onChange={(event) => setMessage(event.target.value)} />
            </Field>
            {said !== null && (
              <Alert tone="bad" title={t('stash.failed')}>
                <span className="whitespace-pre-wrap">{said}</span>
              </Alert>
            )}
          </DialogBody>
          <DialogActions>
            <DialogClose render={<Button variant="ghost" />}>{t('stash.cancel')}</DialogClose>
            <Button type="submit" variant="primary" disabled={busy}>
              {t('stash.stash')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}

/** The stash entry `commit` is, if it is one. */
export function useStashEntry(path: string, commit: string): Stash | null {
  const sidebar = useSidebar(path)
  if (sidebar.state !== 'read') return null
  return sidebar.value.stashes.find((stash) => stash.commit === commit) ?? null
}

/**
 * What can be done to a stash entry, over what it holds: apply it and keep
 * it, apply it and drop it, or drop it without applying - which loses it, and
 * is asked about first. A change that meets one the working tree has is a
 * conflict git leaves in the files, and a pop that met one keeps the entry.
 */
export function StashBar({ path, stash }: { path: string; stash: Stash }) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const [asking, setAsking] = useState(false)

  const act = (action: StashAction) => {
    setBusy(true)
    setSaid(null)
    api.stashAct(path, stash, action).then(
      () => setBusy(false),
      (cause: unknown) => {
        setBusy(false)
        setSaid(failure(cause))
      },
    )
  }

  return (
    <div className="flex shrink-0 flex-col gap-1.5 border-t border-line bg-raise px-3 py-1.5">
      <div className="flex min-w-0 items-center gap-2 text-xs">
        <Archive aria-hidden className="size-3.5 shrink-0 text-dim" />
        <span className="shrink-0 font-mono text-2xs text-dim">{`stash@{${String(stash.index)}}`}</span>
        <span className="min-w-0 truncate text-dim">{t('stash.entryBody')}</span>
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          <Button size="xs" variant="soft" disabled={busy} onClick={() => act('apply')} title={t('stash.applyHelp')}>
            {t('stash.apply')}
          </Button>
          <Button size="xs" variant="ghost" disabled={busy} onClick={() => act('pop')} title={t('stash.popHelp')}>
            {t('stash.pop')}
          </Button>
          <Button size="xs" variant="danger" disabled={busy} onClick={() => setAsking(true)}>
            {t('stash.drop')}
          </Button>
        </span>
      </div>
      {said !== null && (
        <p role="alert" className="max-h-24 overflow-y-auto text-2xs whitespace-pre-wrap text-bad">
          {said}
        </p>
      )}
      <ConfirmDiscard
        open={asking}
        onOpenChange={setAsking}
        title={t('stash.dropTitle', { name: `stash@{${String(stash.index)}}` })}
        body={t('stash.dropBody', { message: stash.message })}
        confirm={t('stash.drop')}
        onConfirm={() => {
          setAsking(false)
          act('drop')
        }}
      />
    </div>
  )
}
