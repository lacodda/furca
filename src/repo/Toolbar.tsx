import { useEffect, useState, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronUp, TriangleAlert } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { SearchField } from '@/components/ui/search-field'
import { Select, SelectGroup, SelectGroupLabel, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Spinner } from '@/components/ui/spinner'
import type { Search } from '@/lib/graph'
import type { Sidebar } from '@/lib/generated/Sidebar'
import type { Standing } from '@/lib/generated/Standing'
import { matchNumber } from '@/lib/graph-labels'
import { aheadBehind } from '@/lib/names'
import { useStanding } from '@/lib/repos'
import { counted, useWorkingCopy } from '@/lib/working'
import { ReachButtons } from './Reach'
import { StashButton } from './Stash'

/** How long typing pauses before the search starts. */
const PAUSE = 150

/** The value the branch filter has for every branch. */
const EVERYTHING = '*'

export interface ToolbarProps {
  path: string
  sidebar: Sidebar | null
  branch: string | null
  onBranch: (branch: string | null) => void
  query: string
  onQuery: (query: string) => void
  search: Search | null
  selected: number | null
  onStep: (back: boolean) => void
  /** Whether the graph is on show, which is what the search and the choice of
   * history are for. */
  history: boolean
}

/**
 * Above the graph: where the repository stands, which history the graph
 * shows, and the search through it.
 */
export function Toolbar({ path, sidebar, branch, onBranch, query, onQuery, search, selected, onStep, history }: ToolbarProps) {
  const { t } = useTranslation()
  const [typed, setTyped] = useState(query)

  // The search starts when typing pauses, not on every key.
  useEffect(() => {
    if (typed === query) return
    const timer = setTimeout(() => onQuery(typed), PAUSE)
    return () => clearTimeout(timer)
  }, [typed, query, onQuery])

  const keys = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      if (typed !== query) onQuery(typed)
      onStep(event.shiftKey)
    } else if (event.key === 'Escape' && typed !== '') {
      event.preventDefault()
      setTyped('')
      onQuery('')
    }
  }

  return (
    <div className="@container flex h-10 shrink-0 items-center gap-3 border-b border-line bg-raise px-3">
      <Where path={path} />
      {/* The history's search takes the right of the bar: there, the buttons
          go down to their icons where the bar is short, and say their names
          in their titles and to a screen reader still. */}
      <ReachButtons path={path} sidebar={sidebar} labelClass={history ? '@max-[60rem]:sr-only' : '@max-[36rem]:sr-only'} />
      <StashButton path={path} labelClass={history ? '@max-[72rem]:sr-only' : undefined} />
      <div className={history ? 'ml-auto flex items-center gap-2' : 'hidden'}>
        {sidebar && (
          <Select
            value={branch ?? EVERYTHING}
            onValueChange={(value) => onBranch(value === EVERYTHING || value === null ? null : String(value))}
          >
            <SelectTrigger size="sm" aria-label={t('graph.branch')} className="w-48 @max-[72rem]:w-40">
              <SelectValue>
                {(value: unknown) => (value === EVERYTHING ? t('graph.allBranches') : shortRef(String(value)))}
              </SelectValue>
            </SelectTrigger>
            <SelectPopup>
              <SelectItem value={EVERYTHING}>{t('graph.allBranches')}</SelectItem>
              <SelectGroup>
                <SelectGroupLabel>{t('graph.branches')}</SelectGroupLabel>
                {sidebar.refs.branches.map((each) => (
                  <SelectItem key={each.name} value={`refs/heads/${each.name}`}>
                    {each.name}
                  </SelectItem>
                ))}
              </SelectGroup>
              {sidebar.refs.remote_branches.length > 0 && (
                <SelectGroup>
                  <SelectGroupLabel>{t('graph.remotes')}</SelectGroupLabel>
                  {sidebar.refs.remote_branches.map((each) => (
                    <SelectItem key={each.name} value={`refs/remotes/${each.name}`}>
                      {each.name}
                    </SelectItem>
                  ))}
                </SelectGroup>
              )}
            </SelectPopup>
          </Select>
        )}
        <SearchField
          value={typed}
          onValueChange={setTyped}
          onKeyDown={keys}
          clearLabel={t('graph.clear')}
          placeholder={t('graph.searchPlaceholder')}
          aria-label={t('graph.search')}
          shortcut={{ id: 'graph.search', label: t('graph.search'), keys: 'Mod+F', whileTyping: true }}
          className="w-64 @max-[72rem]:w-48"
        />
        <Matches search={search} selected={selected} />
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('graph.previous')}
          disabled={!search || search.rows.length === 0}
          onClick={() => onStep(true)}
        >
          <ChevronUp aria-hidden />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={t('graph.next')}
          disabled={!search || search.rows.length === 0}
          onClick={() => onStep(false)}
        >
          <ChevronDown aria-hidden />
        </Button>
      </div>
    </div>
  )
}

/** `refs/heads/main` as `main`, `refs/remotes/origin/main` as `origin/main`. */
function shortRef(name: string): string {
  return name.replace(/^refs\/(heads|remotes)\//, '')
}

/** How the search stands: which match is chosen of how many, while it is
 * still looking too. */
function Matches({ search, selected }: { search: Search | null; selected: number | null }) {
  const { t } = useTranslation()
  if (!search) return null
  const count = search.rows.length
  const at = matchNumber(search.rows, selected)
  const looking = search.searching.size > 0
  return (
    <span role="status" className="flex min-w-24 items-center gap-1.5 text-xs whitespace-nowrap text-dim">
      {looking && <Spinner size="sm" label={t('graph.searching')} />}
      {count === 0
        ? looking
          ? t('graph.searching')
          : t('graph.none')
        : at > 0
          ? t('graph.matchOf', { at, count })
          : t('graph.found', { count })}
      {search.failed !== null && (
        <TriangleAlert aria-label={search.failed} className="size-3.5 text-warn">
          <title>{search.failed}</title>
        </TriangleAlert>
      )}
    </span>
  )
}

/** Where the repository stands, in one line: the branch, how far it is from
 * its upstream, and what is not committed or put away. */
function Where({ path }: { path: string }) {
  const { t } = useTranslation()
  const read = useStanding(path)
  // Heard as it happens, where the standing is read now and then.
  const copy = useWorkingCopy(path)
  if (read.state !== 'read') return <span className="text-sm text-dim">{read.state === 'failed' ? read.reason : ''}</span>
  const standing: Standing = read.value
  const live = copy.state === 'read' ? counted(copy.status, copy.counts) : null
  const changed = live?.changed ?? standing.changed
  const untracked = live?.untracked ?? standing.untracked
  const sync = standing.upstream ? aheadBehind(standing.upstream) : undefined
  const parts = [
    sync === null
      ? t('standing.gone')
      : sync && (sync.ahead > 0 || sync.behind > 0)
        ? [sync.ahead > 0 && `↑${String(sync.ahead)}`, sync.behind > 0 && `↓${String(sync.behind)}`].filter(Boolean).join(' ')
        : null,
    changed > 0 ? t('standing.changed', { count: changed }) : null,
    untracked > 0 ? t('standing.untracked', { count: untracked }) : null,
    standing.stashes > 0 ? t('standing.stashes', { count: standing.stashes }) : null,
  ].filter((part): part is string => typeof part === 'string' && part !== '')

  return (
    <div className="flex min-w-0 items-baseline gap-2 text-sm">
      <span className="max-w-48 shrink-0 truncate font-semibold" title={standing.upstream?.name}>
        {standing.branch ?? t('standing.detached')}
      </span>
      {parts.length > 0 && <span className="truncate text-xs text-dim">{parts.join(' · ')}</span>}
      {standing.operation && (
        <span className="truncate text-xs text-warn" title={t('standing.operationBody')}>
          {t(`standing.operation.${standing.operation}`)}
        </span>
      )}
    </div>
  )
}
