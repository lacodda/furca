import { useTranslation } from 'react-i18next'
import { FolderGit2, Layers } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Kbd } from '@/components/ui/kbd'
import { RowButton } from '@/components/ui/list-row'
import { ProductMark } from '@/components/ui/product-mark'
import type { Line } from '@/lib/generated/Line'
import type { WindowState } from '@/lib/generated/WindowState'
import type { OpenActions } from '@/shell/OpenMenu'

/**
 * The window with no tab open: the three ways in, side by side - a folder,
 * one opened lately, a set of them. The `+` in the bar offers the same, so
 * nothing here is the only way to anything.
 */
export function Home({
  state,
  line,
  actions,
}: {
  state: WindowState
  line: Line | null
  actions: OpenActions
}) {
  const { t } = useTranslation()
  const sessions = state.sessions.length > 0 || line?.state === 'kept'

  return (
    <div className="flex h-full items-center justify-center overflow-auto p-8">
      <div className="flex w-full max-w-md flex-col gap-8">
        <EmptyState
          mark={<ProductMark product="furca" size={64} />}
          title={t('home.title')}
          body={t('home.body')}
          action={
            <div>
              <Button variant="primary" onClick={actions.browse}>
                {t('open.browse')}
                <Kbd keys="Mod+O" aria-hidden />
              </Button>
            </div>
          }
        />

        {state.recent.length > 0 && (
          <section aria-labelledby="home-recent" className="flex flex-col gap-1">
            <h2 id="home-recent" className="caption px-2.5">
              {t('open.recent')}
            </h2>
            {state.recent.map((tab) => (
              <RowButton
                key={tab.path}
                start={<FolderGit2 aria-hidden />}
                description={tab.path}
                onClick={() => actions.open(tab.path)}
              >
                {tab.name}
              </RowButton>
            ))}
          </section>
        )}

        {sessions && (
          <section aria-labelledby="home-sessions" className="flex flex-col gap-1">
            <h2 id="home-sessions" className="caption px-2.5">
              {t('open.sessions')}
            </h2>
            {line?.state === 'kept' && (
              <RowButton
                start={<Layers aria-hidden />}
                description={t('open.lineHint')}
                end={line.tabs.length}
                onClick={actions.openLine}
              >
                {t('open.line')}
              </RowButton>
            )}
            {state.sessions.map((session) => (
              <RowButton
                key={session.name}
                start={<Layers aria-hidden />}
                end={session.tabs.length}
                onClick={() => actions.openSession(session.name)}
              >
                {session.name}
              </RowButton>
            ))}
          </section>
        )}
      </div>
    </div>
  )
}
