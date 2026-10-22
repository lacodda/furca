import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogClose,
  DialogHeader,
  DialogPopup,
  DialogTitle,
} from '@/components/ui/dialog'
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import type { Session } from '@/lib/generated/Session'

/**
 * The name the open tabs are saved under. A name that is taken replaces that
 * session, and the dialog says so before it happens rather than after.
 */
export function SaveSession({
  open,
  sessions,
  onOpenChange,
  onSave,
}: {
  open: boolean
  sessions: Session[]
  onOpenChange: (open: boolean) => void
  onSave: (name: string) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const trimmed = name.trim()
  const taken = sessions.some((session) => session.name === trimmed)

  function submit(event: FormEvent) {
    event.preventDefault()
    if (trimmed === '') return
    onSave(trimmed)
    setName('')
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup size="sm">
        <form onSubmit={submit}>
          <DialogHeader>
            <DialogTitle>{t('session.title')}</DialogTitle>
          </DialogHeader>
          <DialogBody>
            <Field label={t('session.name')} help={taken ? t('session.replaces') : t('session.help')}>
              <Input autoFocus value={name} onChange={(event) => setName(event.target.value)} />
            </Field>
          </DialogBody>
          <DialogActions>
            <DialogClose render={<Button variant="ghost" />}>{t('session.cancel')}</DialogClose>
            <Button type="submit" variant="primary" disabled={trimmed === ''}>
              {taken ? t('session.replace') : t('session.save')}
            </Button>
          </DialogActions>
        </form>
      </DialogPopup>
    </Dialog>
  )
}
