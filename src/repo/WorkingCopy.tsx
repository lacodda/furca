import { useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Minus, Plus, TriangleAlert, Undo2 } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import type { TreeNode } from '@/components/ui/tree-rows'
import { api, failure } from '@/lib/api'
import { fileTree, folderIds } from '@/lib/changes'
import type { Conflicted } from '@/lib/generated/Conflicted'
import type { Counts } from '@/lib/generated/Counts'
import type { FileChange } from '@/lib/generated/FileChange'
import type { Layout } from '@/lib/generated/Layout'
import type { Status } from '@/lib/generated/Status'
import type { Warning } from '@/lib/generated/Warning'
import { actOn, discardOf, readAs, rowToShow, useWorkingCopy, type Area, type Chosen, type Discard } from '@/lib/working'
import { Composer } from './Composer'
import { ConfirmDiscard } from './ConfirmDiscard'
import { DiffPane, type LineActions } from './DiffPane'
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
  const { status, counts, warnings } = copy
  return <Lists path={path} status={status} counts={counts} warnings={warnings} layout={layout} onLayout={onLayout} />
}

function Lists({
  path,
  status,
  counts,
  warnings,
  layout,
  onLayout,
}: {
  path: string
  status: Status
  counts: Counts
  warnings: readonly Warning[]
  layout: Layout
  onLayout: (layout: Layout) => void
}) {
  const { t } = useTranslation()
  const copy = useWorkingCopy(path)
  const [refused, setRefused] = useState<string | null>(null)
  const clean = status.conflicted.length + counts.staged + counts.unstaged + counts.untracked === 0
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

  // A change to the index is heard back as the list moving, so all that is left
  // here is to say what went wrong.
  const heard = (change: Promise<unknown>) => {
    setRefused(null)
    change.catch((cause: unknown) => {
      setRefused(failure(cause))
    })
  }

  const act = (id: string) => {
    const what = actOn(id, status)
    if (what === null) return
    if (what.kind === 'stage') heard(api.stagePaths(path, what.paths))
    else if (what.kind === 'stage-list') heard(api.stageList(path, what.list))
    else if (what.kind === 'unstage') heard(api.unstagePaths(path, what.paths))
    else heard(api.unstageAll(path))
  }

  // What a row would throw away, once the window has asked.
  const [asked, setAsked] = useState<Discard | null>(null)
  const discard = (id: string) => {
    const what = discardOf(id, status, counts)
    if (what !== null) setAsked(what)
  }
  const discarded = (what: Discard) => {
    setAsked(null)
    if (what.kind === 'list') heard(api.discardList(path, what.list))
    else heard(api.discardPaths(path, what.tracked, what.untracked))
  }

  const file = shown === null || shown.area === 'conflicted' ? null : status[shown.area].find((each) => each.path === shown.path)

  // Only what changed in place has hunks and lines to take apart: a file added
  // or deleted whole is one hunk, which its row already moves.
  let lines: LineActions | undefined
  if (file?.status === 'modified' && shown?.area === 'unstaged') {
    lines = {
      staged: false,
      move: (picks) => heard(api.stageLines(path, file, picks)),
      discard: (picks) => heard(api.discardLines(path, file, picks)),
    }
  } else if (file?.status === 'modified' && shown?.area === 'staged') {
    lines = { staged: true, move: (picks) => heard(api.unstageLines(path, file, picks)) }
  }

  return (
    <Splitter
      orientation="horizontal"
      defaultSizes={layout.files}
      onSizesChange={(sizes) => onLayout({ ...layout, files: sizes })}
      className="h-full min-h-0"
    >
      <SplitterPane min={15} max={60}>
        <div className="flex h-full min-h-0 flex-col">
          <div className="min-h-16 flex-1">
            <RowTree path={path} status={status} counts={counts} chosen={shown} onChoose={choose} onAct={act} onDiscard={discard} />
          </div>
          {refused !== null && (
            <p role="alert" className="max-h-24 shrink-0 overflow-y-auto border-t border-line px-2 py-1 text-2xs whitespace-pre-wrap text-bad">
              {refused}
            </p>
          )}
          <Composer key={path} path={path} staged={counts.staged} conflicted={status.conflicted.length} warnings={warnings} />
        </div>
      </SplitterPane>
      <SplitterHandle label={t('details.resizeFiles')} />
      <SplitterPane min={30}>
        {clean ? (
          <div className="flex h-full items-center justify-center p-6 text-center">
            <EmptyState plain title={t('changes.clean')} body={t('changes.cleanBody')} />
          </div>
        ) : shown?.area === 'conflicted' ? (
          <ConflictNote conflict={status.conflicted.find((each) => each.path === shown.path)} />
        ) : file !== null && file !== undefined && shown !== null ? (
          <DiffPane
            path={path}
            scope={readAs(copy, shown)}
            file={file}
            layout={layout.diff}
            onLayout={(diff) => onLayout({ ...layout, diff })}
            lines={lines}
          />
        ) : null}
      </SplitterPane>
      <ConfirmDiscard
        open={asked !== null}
        onOpenChange={(open) => {
          if (!open) setAsked(null)
        }}
        title={
          asked === null
            ? ''
            : asked.kind === 'list'
              ? asked.list === 'untracked'
                ? t('changes.deleteTitle', { count: asked.count })
                : t('changes.discardTitle', { count: asked.count })
              : asked.untracked.length > 0
                ? t('changes.deleteTitle', { count: asked.count })
                : t('changes.discardTitle', { count: asked.count })
        }
        body={t('changes.discardBody')}
        confirm={asked !== null && (asked.kind === 'list' ? asked.list === 'untracked' : asked.untracked.length > 0) ? t('changes.delete') : t('changes.discard')}
        onConfirm={() => {
          if (asked !== null) discarded(asked)
        }}
      />
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
  onAct,
  onDiscard,
}: {
  path: string
  status: Status
  counts: Counts
  chosen: Chosen | null
  onChoose: (row: Chosen) => void
  onAct: (id: string) => void
  onDiscard: (id: string) => void
}) {
  const { t } = useTranslation()
  const [closed, setClosed] = useState<ReadonlySet<string>>(() => closedFolders.get(path) ?? new Set())

  // Read when pressed, not reasons to build the rows again.
  const acting = useRef(onAct)
  const discarding = useRef(onDiscard)
  useEffect(() => {
    acting.current = onAct
    discarding.current = onDiscard
  })

  const { nodes, folders } = useMemo(() => {
    const act = (id: string) => acting.current(id)
    const discard = (id: string) => discarding.current(id)
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
      const staged = area === 'staged'
      const end = (id: string) => (
        <>
          {!staged && <DiscardAction id={id} untracked={area === 'untracked'} onDiscard={discard} />}
          <RowAction id={id} staged={staged} onAct={act} />
        </>
      )
      const children = fileNodes(tree, `${area}|`, end)
      if (count > files.length) {
        children.push({
          id: `${area}|more`,
          label: <span className="text-xs text-dim">{t('changes.more', { count: count - files.length })}</span>,
        })
      }
      const sectionEnd = (
        <>
          {!staged && <DiscardAction id={area} untracked={area === 'untracked'} onDiscard={discard} always />}
          <RowAction id={area} staged={staged} onAct={act} always />
        </>
      )
      nodes.push({ id: area, label: <SectionLabel title={title} count={count} end={sectionEnd} />, children })
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
      onAct={onAct}
      onDiscard={onDiscard}
      selected={chosen === null ? undefined : `${chosen.area}|f:${chosen.path}`}
      onSelect={(id) => {
        const row = /^(conflicted|staged|unstaged|untracked)\|f:(.*)$/s.exec(id)
        if (row?.[1] !== undefined && row[2] !== undefined) onChoose({ area: row[1] as Area, path: row[2] })
      }}
    />
  )
}

function SectionLabel({ title, count, end }: { title: string; count: number; end?: ReactNode }) {
  return (
    <span className="flex min-w-0 items-center gap-2 font-medium">
      <span className="truncate">{title}</span>
      <span className="ml-auto shrink-0 font-mono text-2xs font-normal text-dim">{count}</span>
      {end}
    </span>
  )
}

/** The button at a row's edge that moves it across: to the index, or out of it.
 * The tree is one tab stop and Space does the same from the keyboard, so this is
 * for the pointer, and comes up with it; a list's own row always shows it. */
function RowAction({ id, staged, onAct, always = false }: { id: string; staged: boolean; onAct: (id: string) => void; always?: boolean }) {
  const { t } = useTranslation()
  const label = staged ? t('changes.unstage') : t('changes.stage')
  return (
    <Button
      type="button"
      variant="icon"
      size="icon-xs"
      tabIndex={-1}
      aria-label={label}
      title={label}
      className={cn('shrink-0 font-sans', !always && 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100')}
      onClick={(event: MouseEvent) => {
        event.stopPropagation()
        onAct(id)
      }}
    >
      {staged ? <Minus aria-hidden /> : <Plus aria-hidden />}
    </Button>
  )
}

/** The button beside it that throws a row's change away: puts the file back as
 * the index has it, or deletes it if git does not track it. Delete does the
 * same from the keyboard, and the window asks first either way. */
function DiscardAction({
  id,
  untracked,
  onDiscard,
  always = false,
}: {
  id: string
  untracked: boolean
  onDiscard: (id: string) => void
  always?: boolean
}) {
  const { t } = useTranslation()
  const label = untracked ? t('changes.delete') : t('changes.discard')
  return (
    <Button
      type="button"
      variant="icon"
      size="icon-xs"
      tabIndex={-1}
      aria-label={label}
      title={label}
      className={cn('shrink-0 font-sans hover:text-bad', !always && 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100')}
      onClick={(event: MouseEvent) => {
        event.stopPropagation()
        onDiscard(id)
      }}
    >
      <Undo2 aria-hidden />
    </Button>
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
