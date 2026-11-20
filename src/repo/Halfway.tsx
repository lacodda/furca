import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { GitMerge } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { bringing } from '@/lib/merge'
import { proceed, useReach } from '@/lib/remote'
import { useHalfway } from '@/lib/repos'
import { useWorkingCopy } from '@/lib/working'
import { ConfirmDiscard } from './ConfirmDiscard'

/**
 * The operation git has halfway in a repository - a merge, a pick, a revert,
 * a rebase that stopped - across the top of the view while it lasts: what it
 * is bringing in, what is left to resolve, and the way on. Continue makes the
 * commit with the message git wrote; Abort puts everything back, after a
 * question. What each means is the core's (ADR 0023).
 */
export function HalfwayBar({ path, inChanges, onResolve }: { path: string; inChanges: boolean; onResolve: () => void }) {
  const { t } = useTranslation()
  const read = useHalfway(path)
  const copy = useWorkingCopy(path)
  const busy = useReach(path).run !== null
  const [aborting, setAborting] = useState(false)
  if (read.state !== 'read' || read.value === null) return null
  const halfway = read.value
  // Heard as files are resolved, where the halfway state is read as refs move.
  const conflicts = copy.state === 'read' ? copy.status.conflicted.length : halfway.conflicts
  const what = bringing(halfway) ?? ''
  const into = halfway.branch ?? 'HEAD'
  const subject = halfway.operation === 'merge' ? halfway.message : halfway.commit?.subject

  return (
    <div
      role="region"
      aria-label={t('merge.barLabel')}
      className="@container flex min-h-9 shrink-0 items-center gap-3 border-b border-line bg-warn-soft px-3 py-1"
    >
      <GitMerge aria-hidden className="size-4 shrink-0 text-warn" />
      <div className="flex min-w-0 items-baseline gap-2 text-xs">
        <span className="truncate font-semibold" title={subject ?? undefined}>
          {t(`merge.halfway.${halfway.operation}`, { what, into })}
        </span>
        <span className="shrink-0 text-dim">
          {halfway.operation === 'bisect'
            ? t('merge.bisectBody')
            : conflicts > 0
              ? t('merge.conflicts', { count: conflicts })
              : t('merge.nothingLeft')}
        </span>
      </div>
      <div className="ml-auto flex shrink-0 items-center gap-1.5">
        {conflicts > 0 && !inChanges && (
          <Button size="xs" variant="soft" onClick={onResolve}>
            {t('merge.resolve')}
          </Button>
        )}
        {halfway.can_skip && (
          <Button size="xs" variant="ghost" disabled={busy} title={t('merge.skipHelp')} onClick={() => void proceed(path, 'skip')}>
            {t('merge.skip')}
          </Button>
        )}
        <Button size="xs" variant="ghost" disabled={busy} onClick={() => setAborting(true)}>
          {t('merge.abort')}
        </Button>
        {halfway.can_continue && (
          <Button
            size="xs"
            variant="primary"
            disabled={busy || conflicts > 0}
            title={conflicts > 0 ? t('merge.continueBlocked') : t('merge.continueHelp')}
            onClick={() => void proceed(path, 'continue')}
          >
            {t('merge.continue')}
          </Button>
        )}
      </div>
      <ConfirmDiscard
        open={aborting}
        onOpenChange={setAborting}
        title={t(`merge.abortTitle.${halfway.operation}`)}
        body={t('merge.abortBody')}
        confirm={t('merge.abortConfirm')}
        onConfirm={() => {
          setAborting(false)
          void proceed(path, 'abort')
        }}
      />
    </div>
  )
}
