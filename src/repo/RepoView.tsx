import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { FilePen, History, Rocket, SquareTerminal } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Button } from '@/components/ui/button'
import { RowButton } from '@/components/ui/list-row'
import { ariaKeyShortcuts, keysOf } from '@/components/ui/shortcut'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import type { Layout } from '@/lib/generated/Layout'
import type { Tab } from '@/lib/generated/Tab'
import { positionOf, rowAt, searchGraph, showBranch, useGraph } from '@/lib/graph'
import { labelsByCommit, nextMatch, type Label } from '@/lib/graph-labels'
import { readState, useRelease } from '@/lib/release'
import { failed, useConsole } from '@/lib/console'
import { progressWords, summary, useReach } from '@/lib/remote'
import { useMaintenance, useSidebar } from '@/lib/repos'
import { counted, useWorkingCopy } from '@/lib/working'
import { ConsolePanel } from './ConsolePanel'
import { Details } from './Details'
import { GraphView } from './GraphView'
import { CommitMenu } from './Refs'
import { ReachFailure } from './Reach'
import { ReleasePanel } from './ReleasePanel'
import { Sidebar } from './Sidebar'
import { Toolbar } from './Toolbar'
import { WorkingCopyView } from './WorkingCopy'

/** What the middle of the window shows of a repository: its history, what is
 * not committed yet, or its release. */
type View = 'history' | 'changes' | 'release'

/** What each tab had chosen and was searching for, kept while the window
 * is: going back to a tab finds it as it was left. */
const kept = new Map<string, { id: string | null; query: string; view: View }>()

const NO_LABELS: ReadonlyMap<string, Label[]> = new Map()
const NO_ROWS: ReadonlySet<number> = new Set()

export interface RepoViewProps {
  tab: Tab
  /** How the view is divided, kept by the window's store. */
  layout: Layout
  onLayout: (layout: Layout) => void
}

/**
 * One open repository: the left column of its refs, and beside it either the
 * graph of its history with the chosen commit's details under it, or its
 * working copy; a status line along the foot for what the window is doing on
 * its behalf.
 */
export function RepoView({ tab, layout, onLayout }: RepoViewProps) {
  const { t } = useTranslation()
  const path = tab.path
  const graph = useGraph(path)
  const sidebar = useSidebar(path)
  const [selected, setSelected] = useState<number | null>(null)
  // Read by callbacks that outlive a render: a search answering later.
  const selectedNow = useRef<number | null>(null)
  useEffect(() => {
    selectedNow.current = selected
  }, [selected])
  const [query, setQuery] = useState(() => kept.get(path)?.query ?? '')
  const [view, setView] = useState<View>(() => kept.get(path)?.view ?? 'history')
  const copy = useWorkingCopy(path)
  const changes = copy.state === 'read' ? counted(copy.status, copy.counts).total : null
  const release = useRelease(path)
  const releasable = release.state?.configured === true
  useEffect(() => {
    void readState(path)
  }, [path])
  // The commit chosen, which outlives its row: a graph begun again - after
  // a commit, a fetch, another branch shown - puts it somewhere else.
  const chosen = useRef<string | null>(kept.get(path)?.id ?? null)

  const data = sidebar.state === 'read' ? sidebar.value : null
  const labels = useMemo(() => (data ? labelsByCommit(data) : NO_LABELS), [data])
  const generation = graph.state?.generation
  const head = data?.head.commit ?? null

  const select = useCallback(
    (row: number) => {
      setSelected(row)
      chosen.current = rowAt(path, row)?.commit.id ?? null
    },
    [path],
  )

  // A row chosen before its page was read learns its commit once it is.
  useEffect(() => {
    if (selected !== null && chosen.current === null) chosen.current = rowAt(path, selected)?.commit.id ?? null
  }, [path, selected, graph.version])

  // A new graph: find the chosen commit in it - or, before one is chosen,
  // the commit HEAD is on, as Fork opens a repository.
  useEffect(() => {
    if (generation === undefined) return
    const id = chosen.current ?? head
    if (id === null) {
      setSelected(null)
      return
    }
    let current = true
    void positionOf(path, id).then(async (row) => {
      // A commit chosen that the graph no longer has - a stash entry popped or
      // dropped - gives way to the one HEAD is on.
      if (row === null && id !== head && head !== null) {
        const atHead = await positionOf(path, head)
        if (!current) return
        setSelected(atHead)
        chosen.current = atHead === null ? null : head
        return
      }
      if (!current) return
      setSelected(row)
      if (row !== null) chosen.current = id
    })
    return () => {
      current = false
    }
  }, [path, generation, head])

  useEffect(() => {
    kept.set(path, { id: chosen.current, query, view })
  })

  // Search the graph again whenever the query or the graph changes. The
  // first answer moves to the first match from where the reader is - which
  // stays put if it is a match; after that the reader steps through them.
  useEffect(() => {
    void searchGraph(path, query, (rows) => {
      const from = selectedNow.current
      const at = nextMatch(rows, from === null ? null : from - 1, false)
      if (at !== null) select(at)
    })
  }, [path, query, generation, select])

  const search = graph.search

  const step = (back: boolean) => {
    if (!search) return
    const at = nextMatch(search.rows, selected, back)
    if (at !== null) select(at)
  }

  const jump = (commit: string) => {
    setView('history')
    void positionOf(path, commit).then((row) => {
      if (row !== null) select(row)
    })
  }

  // Read when pressed: a layout that moved is not a reason to listen again.
  const toggleConsole = useRef(() => {
    /* set below, once there is a layout to toggle */
  })
  useEffect(() => {
    toggleConsole.current = () => onLayout({ ...layout, console_open: !layout.console_open })
  })

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const keys = keysOf(event)
      if (keys === 'Mod+L' || keys === 'Mod+H') {
        event.preventDefault()
        setView(keys === 'Mod+L' ? 'changes' : 'history')
      } else if (keys === 'Mod+R') {
        // Taken with no panel too: left alone, it reloads the page.
        event.preventDefault()
        if (releasable) setView('release')
      } else if (keys === 'Mod+J') {
        event.preventDefault()
        toggleConsole.current()
      } else if (keys === 'Mod+F' && view !== 'history') {
        // The search belongs to the history, and is hidden with it: its key
        // goes there first, and the field can take the focus once it shows.
        setView('history')
        requestAnimationFrame(() => document.querySelector<HTMLInputElement>('input[type=search]')?.focus())
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [releasable, view])

  const found = useMemo(() => (search ? new Set(search.rows) : NO_ROWS), [search])
  const consoleOpen = layout.console_open
  // Read on every render: the row's page arrives after the row is chosen.
  const commit = selected === null ? null : (rowAt(path, selected)?.commit.id ?? null)

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Splitter
        orientation="vertical"
        defaultSizes={layout.console}
        onSizesChange={(sizes) => onLayout({ ...layout, console: sizes })}
        className="min-h-0 flex-1"
      >
       <SplitterPane min={20}>
       <div className="flex h-full min-h-0">
        <aside className="flex w-64 shrink-0 flex-col border-r border-line bg-raise">
          <nav aria-label={t('views.label')} className="flex shrink-0 flex-col gap-0.5 border-b border-line p-1.5">
            <RowButton
              selected={view === 'changes'}
              start={<FilePen aria-hidden />}
              end={changes !== null && changes > 0 ? changes : undefined}
              aria-keyshortcuts={ariaKeyShortcuts('Mod+L')}
              onClick={() => setView('changes')}
            >
              {t('views.changes')}
            </RowButton>
            <RowButton
              selected={view === 'history'}
              start={<History aria-hidden />}
              aria-keyshortcuts={ariaKeyShortcuts('Mod+H')}
              onClick={() => setView('history')}
            >
              {t('views.history')}
            </RowButton>
            {releasable && (
              <RowButton
                selected={view === 'release'}
                start={<Rocket aria-hidden />}
                end={release.running ? <Spinner size="sm" label={t('views.releasing')} /> : undefined}
                aria-keyshortcuts={ariaKeyShortcuts('Mod+R')}
                onClick={() => setView('release')}
              >
                {t('views.release')}
              </RowButton>
            )}
          </nav>
          <div className="min-h-0 flex-1">
            <Sidebar path={path} onJump={jump} />
          </div>
        </aside>
        <section className="flex min-w-0 flex-1 flex-col">
          <Toolbar
            path={path}
            sidebar={data}
            branch={graph.state?.branch ?? null}
            onBranch={(branch) => void showBranch(path, branch)}
            query={query}
            onQuery={setQuery}
            search={search}
            selected={selected}
            onStep={step}
            history={view === 'history'}
          />
          <ReachFailure path={path} />
          {view === 'changes' && (
            <div className="min-h-0 flex-1">
              <WorkingCopyView path={path} layout={layout} onLayout={onLayout} />
            </div>
          )}
          {view === 'release' && (
            <div className="min-h-0 flex-1">
              <ReleasePanel path={path} layout={layout} onLayout={onLayout} />
            </div>
          )}
          {/* Kept drawn behind the working copy: coming back finds the graph
              where it was, scrolled and chosen. */}
          <Splitter
            orientation="vertical"
            defaultSizes={layout.details}
            onSizesChange={(sizes) => onLayout({ ...layout, details: sizes })}
            className={view === 'history' ? 'min-h-0 flex-1' : 'hidden'}
          >
            <SplitterPane min={20}>
              <CommitMenu path={path} sidebar={data}>
              <GraphView
                path={path}
                graph={graph}
                labels={labels}
                head={head}
                selected={selected}
                onSelect={select}
                query={search?.query ?? ''}
                found={found}
                inMessage={search?.inMessage ?? NO_ROWS}
              />
              </CommitMenu>
            </SplitterPane>
            <SplitterHandle label={t('details.resizeDetails')} />
            <SplitterPane min={15} className="border-t border-line">
              <Details path={path} commit={commit} layout={layout} onLayout={onLayout} onJump={jump} />
            </SplitterPane>
          </Splitter>
        </section>
       </div>
       </SplitterPane>
       {/* Hidden rather than taken away: the panes of a splitter are its direct
           children, and the view above it must keep what it has drawn. */}
       <SplitterHandle label={t('console.resize')} className={consoleOpen ? undefined : 'hidden'} />
       <SplitterPane min={8} className={cn('border-t border-line', !consoleOpen && 'hidden')}>
        {consoleOpen && <ConsolePanel path={path} />}
       </SplitterPane>
      </Splitter>
      <StatusLine tab={tab} consoleOpen={consoleOpen} onConsole={() => toggleConsole.current()} />
    </div>
  )
}

/** The foot of the window: where the repository is, and the background work
 * on it - git writing a commit-graph is a write the person should be able to
 * see happen. */
function StatusLine({ tab, consoleOpen, onConsole }: { tab: Tab; consoleOpen: boolean; onConsole: () => void }) {
  const { t } = useTranslation()
  const runs = useConsole(tab.path)
  const last = runs.at(-1)
  const step = useMaintenance(tab.path)
  const copy = useWorkingCopy(tab.path)
  const unwatched = copy.state === 'read' ? copy.unwatched : null
  const reach = useReach(tab.path)
  const run = reach.run
  const said = reach.outcome?.ok ? summary(reach.outcome) : null
  const reaching =
    run !== null
      ? [
          run.remote === null ? t(`reach.doing.${run.doing}`) : t(`reach.doingAt.${run.doing}`, { remote: run.remote }),
          run.progress && progressWords(run.progress),
        ]
          .filter(Boolean)
          .join(' · ')
      : said && t(said.key, said.values)

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
      {unwatched !== null && (
        <span role="status" className="shrink-0 text-warn" title={unwatched}>
          {t('status.unwatched')}
        </span>
      )}
      <span role="status" className="ml-auto min-w-0 truncate">
        {reaching}
      </span>
      {work !== null && (
        <span role="status" className={step?.state === 'failed' ? 'shrink-0 text-bad' : 'shrink-0'}>
          {work}
        </span>
      )}
      <Button
        variant="icon"
        size="xs"
        aria-pressed={consoleOpen}
        aria-keyshortcuts={ariaKeyShortcuts('Mod+J')}
        title={t('console.toggle')}
        className={cn('h-5 shrink-0', last !== undefined && failed(last) && 'text-bad')}
        onClick={onConsole}
      >
        <SquareTerminal aria-hidden />
        {t('console.title')}
      </Button>
    </footer>
  )
}
