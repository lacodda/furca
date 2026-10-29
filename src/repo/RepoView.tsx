import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import type { Layout } from '@/lib/generated/Layout'
import type { Tab } from '@/lib/generated/Tab'
import { positionOf, rowAt, searchGraph, showBranch, useGraph } from '@/lib/graph'
import { labelsByCommit, nextMatch, type Label } from '@/lib/graph-labels'
import { useMaintenance, useSidebar } from '@/lib/repos'
import { Details } from './Details'
import { GraphView } from './GraphView'
import { Sidebar } from './Sidebar'
import { Toolbar } from './Toolbar'

/** What each tab had chosen and was searching for, kept while the window
 * is: going back to a tab finds it as it was left. */
const kept = new Map<string, { id: string | null; query: string }>()

const NO_LABELS: ReadonlyMap<string, Label[]> = new Map()
const NO_ROWS: ReadonlySet<number> = new Set()

export interface RepoViewProps {
  tab: Tab
  /** How the view is divided, kept by the window's store. */
  layout: Layout
  onLayout: (layout: Layout) => void
}

/**
 * One open repository: the left column of its refs, the graph of its
 * history with the search above it, the chosen commit's details under the
 * graph, and a status line along the foot for what the window is doing on
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
    void positionOf(path, id).then((row) => {
      if (!current) return
      setSelected(row)
      if (row !== null) chosen.current = id
    })
    return () => {
      current = false
    }
  }, [path, generation, head])

  useEffect(() => {
    kept.set(path, { id: chosen.current, query })
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
    void positionOf(path, commit).then((row) => {
      if (row !== null) select(row)
    })
  }

  const found = useMemo(() => (search ? new Set(search.rows) : NO_ROWS), [search])
  // Read on every render: the row's page arrives after the row is chosen.
  const commit = selected === null ? null : (rowAt(path, selected)?.commit.id ?? null)

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex min-h-0 flex-1">
        <aside className="w-64 shrink-0 border-r border-line bg-raise">
          <Sidebar path={path} onJump={jump} />
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
          />
          <Splitter
            orientation="vertical"
            defaultSizes={layout.details}
            onSizesChange={(sizes) => onLayout({ ...layout, details: sizes })}
            className="min-h-0 flex-1"
          >
            <SplitterPane min={20}>
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
            </SplitterPane>
            <SplitterHandle label={t('details.resizeDetails')} />
            <SplitterPane min={15} className="border-t border-line">
              <Details path={path} commit={commit} layout={layout} onLayout={onLayout} onJump={jump} />
            </SplitterPane>
          </Splitter>
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
