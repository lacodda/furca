import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { TriangleAlert } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import type { TreeNode } from '@/components/ui/tree-rows'
import { fileTree, folderIds } from '@/lib/changes'
import type { Conflicted } from '@/lib/generated/Conflicted'
import type { Counts } from '@/lib/generated/Counts'
import type { FileChange } from '@/lib/generated/FileChange'
import type { Layout } from '@/lib/generated/Layout'
import type { Status } from '@/lib/generated/Status'
import { readAs, rowToShow, useWorkingCopy, type Area, type Chosen } from '@/lib/working'
import { DiffPane } from './DiffPane'
import { FileTree } from './FileTree'
import { fileNodes } from './Files'

/** Which row of each repository was open, kept while the window is. */
const openRow = new Map<string, Chosen>()
/** Which folders of each repository were closed, kept the same way. Closed
 * rather than open: a folder a save brings in starts open. */
const closedFolders = new Map<string, ReadonlySet<string>>()

export interface WorkingCopyProps {
  path: string
  layout: Layout
  onLayout: (layout: Layout) => void
}

/**
 * The files that differ from `HEAD`, and the change of the one chosen, kept
 * up to date as files are saved: the list on the left in the lists git keeps
 * them in, the change on the right, as a commit's is drawn under the graph.
 */
export function WorkingCopyView({ path, layout, onLayout }: WorkingCopyProps) {
  const { t } = useTranslation()
  const copy = useWorkingCopy(path)

  if (copy.state === 'reading') {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('changes.reading')} />
      </div>
    )
  }
  if (copy.state === 'failed') {
    return (
      <div className="p-3">
        <Alert tone="bad" title={t('changes.failed')}>
          {copy.reason}
        </Alert>
      </div>
    )
  }
  const { status, counts } = copy
  const clean = status.conflicted.length + counts.staged + counts.unstaged + counts.untracked === 0
  if (clean) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center">
        <EmptyState plain title={t('changes.clean')} body={t('changes.cleanBody')} />
      </div>
    )
  }
  return <Lists path={path} status={status} counts={counts} layout={layout} onLayout={onLayout} />
}

function Lists({
  path,
  status,
  counts,
  layout,
  onLayout,
}: {
  path: string
  status: Status
  counts: Counts
  layout: Layout
  onLayout: (layout: Layout) => void
}) {
  const { t } = useTranslation()
  const copy = useWorkingCopy(path)
  // The row on show with the lists it was chosen in, which is where the place
  // of a row that has gone is found.
  const [held, setHeld] = useState(() => ({ status, shown: rowToShow(status, openRow.get(path) ?? null) }))
  let shown = held.shown
  if (held.status !== status) {
    shown = rowToShow(status, held.shown, held.status)
    setHeld({ status, shown })
  }
  useEffect(() => {
    if (shown !== null) openRow.set(path, shown)
  })
  const choose = (row: Chosen) => setHeld({ status, shown: row })

  const file = shown === null || shown.area === 'conflicted' ? null : status[shown.area].find((each) => each.path === shown.path)

  return (
    <Splitter
      orientation="horizontal"
      defaultSizes={layout.files}
      onSizesChange={(sizes) => onLayout({ ...layout, files: sizes })}
      className="h-full min-h-0"
    >
      <SplitterPane min={15} max={60}>
        <RowTree path={path} status={status} counts={counts} chosen={shown} onChoose={choose} />
      </SplitterPane>
      <SplitterHandle label={t('details.resizeFiles')} />
      <SplitterPane min={30}>
        {shown?.area === 'conflicted' ? (
          <ConflictNote conflict={status.conflicted.find((each) => each.path === shown.path)} />
        ) : file !== null && file !== undefined && shown !== null ? (
          <DiffPane
            path={path}
            scope={readAs(copy, shown)}
            file={file}
            layout={layout.diff}
            onLayout={(diff) => onLayout({ ...layout, diff })}
          />
        ) : null}
      </SplitterPane>
    </Splitter>
  )
}

/** Every list as one tree with one tab stop, as the left column is: arrows
 * move through all of them. */
function RowTree({
  path,
  status,
  counts,
  chosen,
  onChoose,
}: {
  path: string
  status: Status
  counts: Counts
  chosen: Chosen | null
  onChoose: (row: Chosen) => void
}) {
  const { t } = useTranslation()
  const [closed, setClosed] = useState<ReadonlySet<string>>(() => closedFolders.get(path) ?? new Set())

  const { nodes, folders } = useMemo(() => {
    const nodes: TreeNode[] = []
    const folders: string[] = []
    if (status.conflicted.length > 0) {
      folders.push('conflicted')
      nodes.push({
        id: 'conflicted',
        label: <SectionLabel title={t('changes.conflicted')} count={status.conflicted.length} />,
        children: status.conflicted.map((conflict) => ({
          id: `conflicted|f:${conflict.path}`,
          label: <ConflictRow conflict={conflict} />,
        })),
      })
    }
    const lists: [Exclude<Area, 'conflicted'>, string, readonly FileChange[], number][] = [
      ['staged', t('changes.staged'), status.staged, counts.staged],
      ['unstaged', t('changes.unstaged'), status.unstaged, counts.unstaged],
      ['untracked', t('changes.untracked'), status.untracked, counts.untracked],
    ]
    for (const [area, title, files, count] of lists) {
      if (count === 0) continue
      const tree = fileTree(files)
      folders.push(area, ...folderIds(tree).map((id) => `${area}|d:${id}`))
      const children = fileNodes(tree, `${area}|`)
      if (count > files.length) {
        children.push({
          id: `${area}|more`,
          label: <span className="text-xs text-dim">{t('changes.more', { count: count - files.length })}</span>,
        })
      }
      nodes.push({ id: area, label: <SectionLabel title={title} count={count} />, children })
    }
    return { nodes, folders }
  }, [counts, status, t])

  const open = useMemo(() => new Set(folders.filter((id) => !closed.has(id))), [closed, folders])

  return (
    <FileTree
      label={t('changes.filesLabel')}
      nodes={nodes}
      open={open}
      onOpenChange={(next) => {
        const shut = new Set(folders.filter((id) => !next.has(id)))
        closedFolders.set(path, shut)
        setClosed(shut)
      }}
      selected={chosen === null ? undefined : `${chosen.area}|f:${chosen.path}`}
      onSelect={(id) => {
        const row = /^(conflicted|staged|unstaged|untracked)\|f:(.*)$/s.exec(id)
        if (row?.[1] !== undefined && row[2] !== undefined) onChoose({ area: row[1] as Area, path: row[2] })
      }}
    />
  )
}

function SectionLabel({ title, count }: { title: string; count: number }) {
  return (
    <span className="flex min-w-0 items-center gap-2 font-medium">
      <span className="truncate">{title}</span>
      <span className="ml-auto shrink-0 font-mono text-2xs font-normal text-dim">{count}</span>
    </span>
  )
}

function ConflictRow({ conflict }: { conflict: Conflicted }): ReactNode {
  const { t } = useTranslation()
  return (
    <span className="flex min-w-0 items-center gap-1.5" title={conflict.path}>
      <TriangleAlert aria-hidden className="size-3.5 shrink-0 text-bad" />
      <span className="truncate">{conflict.path}</span>
      <span className="ml-auto shrink-0 pl-2 text-2xs text-dim">{t(`changes.conflict.${conflict.kind}`)}</span>
    </span>
  )
}

function ConflictNote({ conflict }: { conflict: Conflicted | undefined }) {
  const { t } = useTranslation()
  if (conflict === undefined) return null
  return (
    <div className="flex h-full items-center justify-center p-6 text-center">
      <EmptyState
        plain
        title={t('changes.conflictTitle', { kind: t(`changes.conflict.${conflict.kind}`) })}
        body={t('changes.conflictBody')}
      />
    </div>
  )
}
