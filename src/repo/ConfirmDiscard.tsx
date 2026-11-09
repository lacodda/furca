import { useTranslation } from 'react-i18next'
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

/**
 * The question before something is thrown away for good: lines, files, a
 * stash entry. git keeps no copy of a change that was never committed or
 * stashed, so the window asks, says what goes, and the answer that loses it
 * is the one that has to be pressed - a stray click beside it does nothing.
 */
export function ConfirmDiscard({
  open,
  onOpenChange,
  title,
  body,
  confirm,
  onConfirm,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  body: string
  /** What the button that loses it says. */
  confirm: string
  onConfirm: () => void
}) {
  const { t } = useTranslation()
  return (
    <ConfirmDialog open={open} onOpenChange={onOpenChange}>
      <ConfirmDialogPopup size="md">
        <ConfirmDialogHeader>
          <ConfirmDialogTitle className="break-words">{title}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{body}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <ConfirmDialogClose render={<Button variant="ghost" />}>{t('changes.keep')}</ConfirmDialogClose>
          <Button variant="primary" className="bg-bad text-on-bad not-data-disabled:hover:bg-bad/85" onClick={onConfirm}>
            {confirm}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}
