import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert } from '@/components/ui/alert'
import { RelativeTime } from '@/components/ui/relative-time'
import { Spinner } from '@/components/ui/spinner'
import { StatusBadge } from '@/components/ui/status-dot'
import type { Standing } from '@/lib/generated/Standing'
import { aheadBehind, short } from '@/lib/names'
import { useStanding } from '@/lib/repos'

/**
 * Where the repository stands, read on this machine alone: the commit HEAD
 * is on, how far the branch is from its upstream, what is not committed,
 * pushed or stashed away, and how far it has come since its last tag - the
 * row `furca line` prints for it, at the size of a window. The graph takes
 * this place when it arrives; until then this is what opening a repository
 * shows.
 */
export function Overview({ path }: { path: string }) {
  const { t } = useTranslation()
  const read = useStanding(path)

  if (read.state === 'reading') {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('overview.reading')} />
      </div>
    )
  }
  if (read.state === 'failed') {
    return (
      <div className="p-6">
        <Alert tone="bad" title={t('overview.failed')}>
          {read.reason}
        </Alert>
      </div>
    )
  }
  return <Standing standing={read.value} />
}

function Standing({ standing }: { standing: Standing }) {
  const { t } = useTranslation()
  const settled =
    standing.changed === 0 &&
    standing.untracked === 0 &&
    standing.unpushed === 0 &&
    standing.stashes === 0 &&
    standing.operation === null

  return (
    <div className="flex h-full flex-col gap-6 overflow-auto p-6">
      <header className="flex flex-col gap-1">
        <h1 className="text-lg font-semibold">{standing.branch ?? t('overview.detached')}</h1>
        {standing.commit ? (
          <p className="flex flex-wrap items-baseline gap-x-2 text-sm text-dim">
            <span className="text-text">{standing.commit.subject}</span>
            <span className="font-mono text-xs">{short(standing.commit.id)}</span>
            <span>{standing.commit.author.name}</span>
            <RelativeTime value={standing.commit.committer.time} className="text-xs" />
          </p>
        ) : (
          <p className="text-sm text-dim">{t('overview.noCommits')}</p>
        )}
      </header>

      {standing.operation && (
        <Alert tone="warn" title={t(`overview.operation.${standing.operation}`)}>
          {t('overview.operationBody')}
        </Alert>
      )}

      <dl className="grid max-w-xl grid-cols-[minmax(8rem,auto)_1fr] gap-x-6 gap-y-2 text-sm">
        <Row term={t('overview.upstream')}>{upstream(standing, t)}</Row>
        <Row term={t('overview.workingTree')}>
          {standing.changed === 0 && standing.untracked === 0
            ? t('overview.clean')
            : [
                standing.changed > 0 && t('overview.changed', { count: standing.changed }),
                standing.untracked > 0 && t('overview.untracked', { count: standing.untracked }),
              ]
                .filter(Boolean)
                .join(', ')}
        </Row>
        <Row term={t('overview.unpushed')}>
          {standing.unpushed === 0 ? t('overview.nothing') : t('overview.commits', { count: standing.unpushed })}
        </Row>
        <Row term={t('overview.stashes')}>
          {standing.stashes === 0 ? t('overview.nothing') : t('overview.entries', { count: standing.stashes })}
        </Row>
        <Row term={t('overview.tag')}>
          {standing.tag ? (
            <span className="flex flex-wrap items-baseline gap-x-2">
              <span className="font-mono">{standing.tag.name}</span>
              <span className="text-dim">{t('overview.since', { count: standing.tag.since })}</span>
              <RelativeTime value={standing.tag.time} className="text-xs text-dim" />
            </span>
          ) : (
            t('overview.noTag')
          )}
        </Row>
      </dl>

      <div>
        <StatusBadge status={settled ? 'good' : 'warn'}>
          {settled ? t('overview.settled') : t('overview.unsettled')}
        </StatusBadge>
      </div>
    </div>
  )
}

function Row({ term, children }: { term: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-dim">{term}</dt>
      <dd>{children}</dd>
    </>
  )
}

function upstream(standing: Standing, t: ReturnType<typeof useTranslation>['t']): ReactNode {
  if (!standing.upstream) return <span className="text-dim">{t('overview.followsNothing')}</span>
  const sync = aheadBehind(standing.upstream)
  const state =
    sync === null
      ? t('overview.gone')
      : sync.ahead === 0 && sync.behind === 0
        ? t('overview.upToDate')
        : [
            sync.ahead > 0 && t('overview.ahead', { count: sync.ahead }),
            sync.behind > 0 && t('overview.behind', { count: sync.behind }),
          ]
            .filter(Boolean)
            .join(', ')
  return (
    <span className="flex flex-wrap items-baseline gap-x-2">
      <span className="font-mono">{standing.upstream.name}</span>
      <span className="text-dim">{state}</span>
    </span>
  )
}
