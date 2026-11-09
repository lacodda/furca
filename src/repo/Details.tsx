import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Copyable } from '@/components/ui/copyable'
import { EmptyState } from '@/components/ui/empty-state'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import { fileTree, folderIds } from '@/lib/changes'
import { useDetails } from '@/lib/details'
import type { Details as Read } from '@/lib/generated/Details'
import type { FileChange } from '@/lib/generated/FileChange'
import type { Layout } from '@/lib/generated/Layout'
import { short } from '@/lib/names'
import { DiffPane } from './DiffPane'
import { FileTree } from './FileTree'
import { fileNodes } from './Files'
import { StashBar, useStashEntry } from './Stash'

export interface DetailsProps {
  path: string
  /** The commit chosen in the graph; `null` before one is. */
  commit: string | null
  layout: Layout
  onLayout: (layout: Layout) => void
  /** Takes the graph to another commit: a parent named in the header. */
  onJump: (commit: string) => void
}

/** Which file of each commit was open, kept while the window is: going back
 * to a commit finds the file it was left on. */
const openFile = new Map<string, string>()

/**
 * The commit chosen in the graph, as Fork shows it under the graph: who made
 * it and when, its message, the files it changed - binaries and git LFS
 * files with their sizes - and the change of the file chosen among them.
 */
export function Details({ path, commit, layout, onLayout, onJump }: DetailsProps) {
  const { t } = useTranslation()
  const shown = useDetails(path, commit)

  if (shown === null) {
    return <EmptyState plain title={t('details.none')} body={t('details.noneBody')} className="h-full" />
  }
  if (shown.read.state === 'reading') {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('details.reading')} />
      </div>
    )
  }
  if (shown.read.state === 'failed') {
    return (
      <div className="p-3">
        <Alert tone="bad" title={t('details.failed')}>
          {shown.read.reason}
        </Alert>
      </div>
    )
  }
  return (
    <Opened
      key={shown.read.value.commit.id}
      path={path}
      details={shown.read.value}
      stale={shown.stale}
      layout={layout}
      onLayout={onLayout}
      onJump={onJump}
    />
  )
}

function Opened({
  path,
  details,
  stale,
  layout,
  onLayout,
  onJump,
}: {
  path: string
  details: Read
  stale: boolean
  layout: Layout
  onLayout: (layout: Layout) => void
  onJump: (commit: string) => void
}) {
  const { t } = useTranslation()
  const files = details.changes.files
  const [chosen, setChosen] = useState<string | undefined>(
    () => openFile.get(`${path}\n${details.commit.id}`) ?? files[0]?.path,
  )
  const file = files.find((candidate) => candidate.path === chosen) ?? files[0] ?? null
  const stash = useStashEntry(path, details.commit.id)
  const choose = (next: string) => {
    openFile.set(`${path}\n${details.commit.id}`, next)
    setChosen(next)
  }

  return (
    <div className={cn('flex h-full min-h-0 flex-col', stale && 'opacity-60')} aria-busy={stale}>
      <Header details={details} onJump={onJump} />
      {stash !== null && <StashBar key={stash.commit} path={path} stash={stash} />}
      {files.length === 0 ? (
        <EmptyState plain title={t('details.noFiles')} body={t('details.noFilesBody')} className="flex-1" />
      ) : (
        <Splitter
          orientation="horizontal"
          defaultSizes={layout.files}
          onSizesChange={(sizes) => onLayout({ ...layout, files: sizes })}
          className="min-h-0 flex-1 border-t border-line"
        >
          <SplitterPane min={15} max={60}>
            <ChangedFiles files={files} chosen={file?.path} onChoose={choose} />
          </SplitterPane>
          <SplitterHandle label={t('details.resizeFiles')} />
          <SplitterPane min={30}>
            {file !== null && (
              <DiffPane
                path={path}
                scope={details.commit.id}
                file={file}
                layout={layout.diff}
                onLayout={(diff) => onLayout({ ...layout, diff })}
              />
            )}
          </SplitterPane>
        </Splitter>
      )}
    </div>
  )
}

/** Who made the commit and when, its id and parents, and the message. */
function Header({ details, onJump }: { details: Read; onJump: (commit: string) => void }) {
  const { t, i18n } = useTranslation()
  const { commit, message, changes } = details
  const body = message.slice(message.indexOf('\n') === -1 ? message.length : message.indexOf('\n')).trim()
  const when = (seconds: bigint | number) =>
    new Date(Number(seconds) * 1000).toLocaleString(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const added = changes.files.reduce((sum, file) => sum + (file.lines?.added ?? 0), 0)
  const removed = changes.files.reduce((sum, file) => sum + (file.lines?.removed ?? 0), 0)
  const committedApart =
    commit.committer.email !== commit.author.email || commit.committer.name !== commit.author.name

  return (
    <header className="flex shrink-0 flex-col gap-1.5 bg-raise px-3 py-2">
      <div className="flex min-w-0 items-baseline gap-3">
        <h2 className="min-w-0 flex-1 truncate text-sm font-medium" title={commit.subject}>
          {commit.subject}
        </h2>
        <Copyable value={commit.id} label={t('details.copyId')} copiedLabel={t('details.copied')} className="font-mono text-2xs">
          {short(commit.id)}
        </Copyable>
      </div>
      <div className="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-2xs text-dim">
        <span title={commit.author.email}>
          {t('details.by', { name: commit.author.name, date: when(commit.author.seconds) })}
        </span>
        {committedApart && (
          <span title={commit.committer.email}>
            {t('details.committedBy', { name: commit.committer.name, date: when(commit.committer.seconds) })}
          </span>
        )}
        {commit.parents.length > 0 && (
          <span className="flex items-center gap-1">
            {t('details.parents', { count: commit.parents.length })}
            {commit.parents.map((parent) => (
              <Button key={parent} variant="link" className="font-mono" title={t('details.toParent')} onClick={() => onJump(parent)}>
                {short(parent)}
              </Button>
            ))}
          </span>
        )}
        <span className="ml-auto font-mono">
          {t('details.files', { count: changes.files.length })}
          <span className="ml-2 text-good">+{added}</span>
          <span className="ml-1 text-bad">−{removed}</span>
        </span>
      </div>
      {body !== '' && (
        <ScrollArea label={t('details.message')} orientation="vertical" className="max-h-24">
          <p className="whitespace-pre-wrap text-xs text-dim">{body}</p>
        </ScrollArea>
      )}
    </header>
  )
}

/** The files a commit changed, as a tree of folders. */
function ChangedFiles({
  files,
  chosen,
  onChoose,
}: {
  files: readonly FileChange[]
  chosen: string | undefined
  onChoose: (path: string) => void
}) {
  const { t } = useTranslation()
  const tree = useMemo(() => fileTree(files), [files])
  const nodes = useMemo(() => fileNodes(tree), [tree])
  const [open, setOpen] = useState<Set<string>>(() => new Set(folderIds(tree).map((id) => `d:${id}`)))
  return (
    <FileTree
      label={t('details.filesLabel')}
      nodes={nodes}
      open={open}
      onOpenChange={setOpen}
      selected={chosen === undefined ? undefined : `f:${chosen}`}
      onSelect={(id) => {
        if (id.startsWith('f:')) onChoose(id.slice(2))
      }}
    />
  )
}
