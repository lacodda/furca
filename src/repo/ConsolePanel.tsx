import { useLayoutEffect, useRef } from 'react'
import { useTranslation } from 'react-i18next'
import { cn, useLocale } from 'dowel-ui'
import { Copyable } from '@/components/ui/copyable'
import { EmptyState } from '@/components/ui/empty-state'
import { ScrollArea } from '@/components/ui/scroll-area'
import { clock, commandLine, failed, useConsole } from '@/lib/console'
import type { GitRun } from '@/lib/generated/GitRun'

/**
 * Every run of git this window made in the repository, newest at the foot: the
 * command, how long it took, how it ended, and what it was given and what it
 * said. It is what a person would have typed, so anything here can be copied
 * and tried in a terminal. Reads go through gitoxide and are not here (ADR 0001).
 */
export function ConsolePanel({ path }: { path: string }) {
  const { t } = useTranslation()
  const runs = useConsole(path)
  const viewport = useRef<HTMLDivElement>(null)
  // Kept at the foot while it is at the foot, as a terminal does; someone who
  // scrolled up to read something stays where they are.
  const atFoot = useRef(true)
  useLayoutEffect(() => {
    const element = viewport.current
    if (element !== null && atFoot.current) element.scrollTop = element.scrollHeight
  }, [runs])

  if (runs.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-4 text-center">
        <EmptyState plain title={t('console.empty')} body={t('console.emptyBody')} />
      </div>
    )
  }
  return (
    <ScrollArea label={t('console.label')} orientation="vertical" className="h-full" viewportRef={viewport}>
      <ol
        aria-label={t('console.label')}
        className="flex flex-col gap-1 p-2 font-mono text-2xs"
        onWheel={() => {
          const element = viewport.current
          if (element !== null) atFoot.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24
        }}
      >
        {runs.map((run, index) => (
          <Line key={`${String(run.at)}-${String(index)}`} run={run} />
        ))}
      </ol>
    </ScrollArea>
  )
}

function Line({ run }: { run: GitRun }) {
  const { t } = useTranslation()
  const locale = useLocale()
  const bad = failed(run)
  return (
    <li className="flex min-w-0 flex-col gap-0.5">
      <div className="flex min-w-0 items-baseline gap-2">
        <span className="shrink-0 text-faint">{clock(run.at, locale)}</span>
        <Copyable value={commandLine(run)} label={t('console.copy')} copiedLabel={t('console.copied')} className={cn('min-w-0 truncate text-left', bad ? 'text-bad' : 'text-text')}>
          {commandLine(run)}
        </Copyable>
        <span className="ml-auto shrink-0 text-faint">
          {bad && `${run.code === null ? t('console.stopped') : t('console.exit', { code: run.code })} · `}
          {t('console.millis', { millis: run.millis })}
        </span>
      </div>
      {run.input !== '' && <pre className="max-h-24 overflow-auto whitespace-pre-wrap break-words pl-4 text-dim">{run.input}</pre>}
      {run.said !== '' && <pre className={cn('whitespace-pre-wrap break-words pl-4', bad ? 'text-bad' : 'text-dim')}>{run.said}</pre>}
    </li>
  )
}
