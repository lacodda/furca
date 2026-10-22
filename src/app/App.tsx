import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { FolderInput } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { ProductMark } from '@/components/ui/product-mark'
import { ResizeEdges, TitleBar } from '@/components/ui/window-frame'
import { api, failure } from '@/lib/api'
import type { Line } from '@/lib/generated/Line'
import type { Opened } from '@/lib/generated/Opened'
import type { WindowState } from '@/lib/generated/WindowState'
import { forget, listenForMaintenance, readAll, refresh, sidebars } from '@/lib/repos'
import { Home } from '@/home/Home'
import { RepoView } from '@/repo/RepoView'
import { OpenMenu, type OpenActions } from '@/shell/OpenMenu'
import { RepoTabs } from '@/shell/RepoTabs'
import { SaveSession } from '@/shell/SaveSession'
import { Splash } from '@/shell/Splash'

const inTauri = () => '__TAURI_INTERNALS__' in window

type Boot = { state: WindowState } | { error: string }

/**
 * The window: the splash while it opens what was open, then the shell.
 *
 * The splash stays until the tab in front has its column read, so the first
 * frame of the shell is a repository rather than a spinner inside one - the
 * open and the first read are what the splash is there to cover.
 */
export default function App() {
  const { t } = useTranslation()
  const [boot, setBoot] = useState<Boot | null>(null)
  const [status, setStatus] = useState(() => t('splash.opening'))

  useEffect(() => {
    let cancelled = false
    api.windowState().then(
      async (state) => {
        void api.measured('state')
        const front = state.active === null ? undefined : state.tabs[state.active]
        if (front) {
          setStatus(t('splash.reading', { name: front.name }))
          await sidebars.load(front.path)
          void api.measured('read')
        }
        if (!cancelled) setBoot({ state })
      },
      (cause: unknown) => {
        if (!cancelled) setBoot({ error: failure(cause) })
      },
    )
    return () => {
      cancelled = true
    }
  }, [t])

  if (boot === null) return <Splash status={status} />
  if ('error' in boot) {
    return (
      <div className="flex h-full items-center justify-center p-8">
        <EmptyState variant="error" title={t('shell.broken')} body={boot.error} />
      </div>
    )
  }
  return <Shell initial={boot.state} />
}

function Shell({ initial }: { initial: WindowState }) {
  const { t } = useTranslation()
  const [state, setState] = useState(initial)
  const [line, setLine] = useState<Line | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  const [dropping, setDropping] = useState(false)
  const front = state.active === null ? undefined : state.tabs[state.active]

  const apply = useCallback(async (call: Promise<WindowState>) => {
    try {
      setState(await call)
      setNotice(null)
    } catch (cause) {
      setNotice(failure(cause))
    }
  }, [])

  const applyOpened = useCallback(
    async (call: Promise<Opened>) => {
      try {
        const opened = await call
        setState(opened.state)
        setNotice(
          opened.failed.length === 0
            ? null
            : t('shell.notOpened', {
                count: opened.failed.length,
                paths: opened.failed.map((failed) => failed.path).join(', '),
              }),
        )
      } catch (cause) {
        setNotice(failure(cause))
      }
    },
    [t],
  )

  const openPaths = useCallback(
    async (paths: string[]) => {
      const failed: string[] = []
      for (const path of paths) {
        try {
          setState(await api.openTab(path))
        } catch (cause) {
          failed.push(failure(cause))
        }
      }
      setNotice(failed.length === 0 ? null : failed.join('\n'))
    },
    [],
  )

  const browse = useCallback(async () => {
    const path = await open({ directory: true, title: t('open.browse') })
    if (typeof path === 'string') await openPaths([path])
  }, [openPaths, t])

  const close = useCallback(
    (index: number) => {
      const tab = state.tabs[index]
      if (tab) forget(tab.path)
      void apply(api.closeTab(index))
    },
    [apply, state.tabs],
  )

  const activate = useCallback(
    (index: number) => {
      const tab = state.tabs[index]
      if (tab) refresh(tab.path)
      void apply(api.activateTab(index))
    },
    [apply, state.tabs],
  )

  const actions: OpenActions = {
    browse: () => void browse(),
    open: (path) => void openPaths([path]),
    openSession: (name) => void applyOpened(api.openSession(name)),
    openLine: () => void applyOpened(api.openLine()),
    saveSession: () => setSaving(true),
    deleteSession: (name) => void apply(api.deleteSession(name)),
  }

  // After the first frame of the shell: say so to a measurement, then do what
  // must not compete with it - ask the record for the line, let the
  // background work start, and read the tabs behind the one in front.
  //
  // A frame callback runs before the paint and a task queued from it after,
  // so the task is the first moment the frame is on the screen. Cancelled,
  // not guarded: StrictMode mounts twice, and a "once" flag set by the first
  // mount would stop the second - the one that stays.
  useEffect(() => {
    let task: ReturnType<typeof setTimeout> | undefined
    const frame = requestAnimationFrame(() => {
      task = setTimeout(() => {
        void api.measured('first-frame')
        api.line().then(setLine, () => setLine(null))
        for (const tab of initial.tabs) void api.maintain(tab.path)
        void readAll(initial.tabs.map((tab) => tab.path)).then(() => api.measured('settled'))
      }, 0)
    })
    return () => {
      cancelAnimationFrame(frame)
      clearTimeout(task)
    }
  }, [initial.tabs])

  useEffect(() => listenForMaintenance(), [])

  // furca started again with a path hands it to this window, which draws
  // the tabs the store now holds.
  useEffect(() => {
    if (!inTauri()) return
    const stop = listen<Opened>('opened', ({ payload }) => {
      void applyOpened(Promise.resolve(payload))
    })
    return () => {
      void stop.then((unlisten) => unlisten())
    }
  }, [applyOpened])

  // Coming back to the window reads the tab in front again: something may
  // have been committed in a terminal meanwhile, and nothing watches the
  // working tree yet.
  useEffect(() => {
    if (!inTauri() || !front) return
    const path = front.path
    const stop = getCurrentWindow().onFocusChanged(({ payload: focused }) => {
      if (focused) refresh(path)
    })
    return () => {
      void stop.then((unlisten) => unlisten())
    }
  }, [front])

  // A folder dropped on the window opens; a file inside a repository opens
  // the repository around it.
  useEffect(() => {
    if (!inTauri()) return
    const stop = getCurrentWebview().onDragDropEvent(({ payload }) => {
      if (payload.type === 'enter' || payload.type === 'over') setDropping(true)
      else if (payload.type === 'leave') setDropping(false)
      else {
        setDropping(false)
        void openPaths(payload.paths)
      }
    })
    return () => {
      void stop.then((unlisten) => unlisten())
    }
  }, [openPaths])

  // The keys of a tabbed window, read by the key's place rather than its
  // letter, so they work on any keyboard layout.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return
      const count = state.tabs.length
      const at = state.active
      if (event.code === 'KeyO' && !event.shiftKey) {
        event.preventDefault()
        void browse()
      } else if (event.code === 'KeyW' && !event.shiftKey && at !== null) {
        event.preventDefault()
        close(at)
      } else if (event.code === 'Tab' && count > 1 && at !== null) {
        event.preventDefault()
        activate((at + (event.shiftKey ? count - 1 : 1)) % count)
      } else if (/^Digit[1-9]$/.test(event.code) && count > 0) {
        event.preventDefault()
        const digit = Number(event.code.slice(5))
        // Mod+9 is the last tab, as in every browser.
        activate(digit === 9 ? count - 1 : Math.min(digit, count) - 1)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [activate, browse, close, state.active, state.tabs.length])

  return (
    <div className="flex h-full flex-col">
      <ResizeEdges />
      <TitleBar
        mark={<ProductMark product="furca" size={18} />}
        labels={{
          minimize: t('window.minimize'),
          maximize: t('window.maximize'),
          restore: t('window.restore'),
          close: t('window.close'),
        }}
      >
        <RepoTabs
          tabs={state.tabs}
          active={state.active}
          onActivate={activate}
          onClose={close}
          onMove={(from, to) => void apply(api.moveTab(from, to))}
        />
        <OpenMenu state={state} line={line} actions={actions} />
      </TitleBar>

      {notice !== null && (
        <Alert
          tone="bad"
          className="m-2 mb-0 whitespace-pre-line"
          action={
            <Button variant="ghost" size="xs" onClick={() => setNotice(null)}>
              {t('shell.dismiss')}
            </Button>
          }
        >
          {notice}
        </Alert>
      )}

      <main className="relative min-h-0 flex-1">
        {front ? <RepoView key={front.path} tab={front} /> : <Home state={state} line={line} actions={actions} />}
        {dropping && (
          <div className="pointer-events-none absolute inset-2 flex flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed border-accent bg-bg/80 text-sm text-text">
            <FolderInput aria-hidden className="size-6 text-accent" />
            {t('shell.drop')}
          </div>
        )}
      </main>

      <SaveSession
        open={saving}
        sessions={state.sessions}
        onOpenChange={setSaving}
        onSave={(name) => {
          setSaving(false)
          void apply(api.saveSession(name))
        }}
      />
    </div>
  )
}
