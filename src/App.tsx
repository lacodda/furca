import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { open } from '@tauri-apps/plugin-dialog'
import { invoke } from '@tauri-apps/api/core'

/** The shape `open_repository` returns — mirrors furca-core's `HeadSummary`. */
interface HeadSummary {
  branch: string | null
  commit: string | null
  detached: boolean
  upstream: string | null
}

/**
 * The minimal shell: a top bar and a repository summary.
 *
 * This is scaffolding, not the product — it exists to prove the door from the
 * window to furca-core is wired end to end (dialog → invoke → JSON back).
 */
export default function App() {
  const { t } = useTranslation()
  const [head, setHead] = useState<HeadSummary | null>(null)
  const [error, setError] = useState<string | null>(null)

  async function openRepository() {
    const path = await open({ directory: true })
    if (typeof path !== 'string') return

    try {
      const summary = await invoke<HeadSummary>('open_repository', { path })
      setHead(summary)
      setError(null)
    } catch (cause) {
      setHead(null)
      setError(t('shell.failed', { message: String(cause) }))
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-[var(--line)] px-4 py-3">
        <span className="font-semibold">{t('app.name')}</span>
        <button
          type="button"
          onClick={() => void openRepository()}
          className="rounded-md bg-[var(--accent)] px-3 py-1.5 text-sm text-[var(--accent-fg)]"
        >
          {t('shell.openRepository')}
        </button>
      </header>
      <main className="flex flex-1 items-center justify-center p-6 text-sm">
        {error && <p className="text-[var(--danger)]">{error}</p>}
        {!error && !head && <p className="text-[var(--dim)]">{t('shell.noRepository')}</p>}
        {!error && head && (
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
            <dt className="text-[var(--dim)]">{t('shell.branch')}</dt>
            <dd>{head.detached ? t('shell.detached') : (head.branch ?? '—')}</dd>
            <dt className="text-[var(--dim)]">{t('shell.commit')}</dt>
            <dd className="font-mono">{head.commit ?? '—'}</dd>
          </dl>
        )}
      </main>
    </div>
  )
}
