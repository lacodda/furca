import { useTranslation } from 'react-i18next'
import type { Tab } from '@/lib/generated/Tab'
import { useMaintenance } from '@/lib/repos'
import { Overview } from './Overview'
import { Sidebar } from './Sidebar'

/**
 * One open repository: the left column of its refs, what the repository
 * holds beside it, and a status line along the foot for what the window is
 * doing on its behalf.
 */
export function RepoView({ tab }: { tab: Tab }) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-0 flex-1">
        <aside className="w-64 shrink-0 border-r border-line bg-raise">
          <Sidebar path={tab.path} />
        </aside>
        <section className="min-w-0 flex-1">
          <Overview path={tab.path} />
        </section>
      </div>
      <StatusLine tab={tab} />
    </div>
  )
}

/** The foot of the window: where the repository is, and the background work
 * on it - git writing a commit-graph is a write the person should be able to
 * see happen. */
function StatusLine({ tab }: { tab: Tab }) {
  const { t } = useTranslation()
  const step = useMaintenance(tab.path)

  const work =
    step === undefined
      ? null
      : step.state === 'writing'
        ? t('status.writingGraph')
        : step.state === 'written'
          ? t('status.wroteGraph', { seconds: (step.millis / 1000).toFixed(1) })
          : t('status.graphFailed', { reason: step.reason })

  return (
    <footer className="flex h-6 shrink-0 items-center gap-4 border-t border-line bg-raise px-3 text-2xs text-dim">
      <span className="min-w-0 truncate font-mono">{tab.path}</span>
      {work !== null && (
        <span role="status" className={step?.state === 'failed' ? 'ml-auto text-bad' : 'ml-auto'}>
          {work}
        </span>
      )}
    </footer>
  )
}
