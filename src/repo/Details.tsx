import { lazy, Suspense, useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowRight, File, FileBox, FileCode, Folder, Package } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Copyable } from '@/components/ui/copyable'
import { EmptyState } from '@/components/ui/empty-state'
import { ErrorBoundary } from '@/components/ui/error-boundary'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import { TreeView } from '@/components/ui/tree-view'
import type { TreeNode } from '@/components/ui/tree-rows'
import { endingsChange, fileTree, folderIds, isRawBinary, sizeText, storedAs, type FileNode } from '@/lib/changes'
import { markShown, useDetails, useFileDiff } from '@/lib/details'
import type { Details as Read } from '@/lib/generated/Details'
import type { DiffLayout } from '@/lib/generated/DiffLayout'
import type { FileChange } from '@/lib/generated/FileChange'
import type { FileDiff } from '@/lib/generated/FileDiff'
import type { Layout } from '@/lib/generated/Layout'
import { short } from '@/lib/names'

/** CodeMirror, fetched when the first diff is opened; `preloadDiff` fetches
 * it sooner, once the first frame is out of the way. */
const loadCodeDiff = () => import('./CodeDiff')
const CodeDiff = lazy(loadCodeDiff)

export function preloadDiff(): void {
  void loadCodeDiff()
}

/** Starts loading the language of the file at `path` beside the read of
 * its change. */
function prepareLanguage(path: string): void {
  void loadCodeDiff().then((module) => module.prepare(path))
}

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
  const choose = (next: string) => {
    openFile.set(`${path}\n${details.commit.id}`, next)
    setChosen(next)
  }

  return (
    <div className={cn('flex h-full min-h-0 flex-col', stale && 'opacity-60')} aria-busy={stale}>
      <Header details={details} onJump={onJump} />
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
                commit={details.commit.id}
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
  const [open, setOpen] = useState<Set<string>>(() => new Set(folderIds(tree).map((id) => `d:${id}`)))
  return (
    <ScrollArea label={t('details.filesLabel')} orientation="vertical" className="h-full">
      <TreeView
        label={t('details.filesLabel')}
        nodes={nodes(tree)}
        open={open}
        onOpenChange={setOpen}
        selected={chosen === undefined ? undefined : `f:${chosen}`}
        onSelect={(id) => {
          if (id.startsWith('f:')) onChoose(id.slice(2))
        }}
        className="p-1.5"
      />
    </ScrollArea>
  )
}

function nodes(tree: readonly FileNode[]): TreeNode[] {
  return tree.map((node) => {
    const leaf = node.file === undefined ? null : { id: `f:${node.file.path}`, label: <FileRow file={node.file} /> }
    if (node.children.length === 0) return leaf ?? { id: `d:${node.id}`, label: node.label }
    const children = nodes(node.children)
    return {
      id: `d:${node.id}`,
      label: (
        <span className="flex min-w-0 items-center gap-1.5">
          <Folder aria-hidden className="size-3.5 shrink-0 text-dim" />
          <span className="truncate">{node.label}</span>
        </span>
      ),
      children: leaf === null ? children : [leaf, ...children],
    }
  })
}

const STATUS = { added: 'A', deleted: 'D', modified: 'M', renamed: 'R', copied: 'C' } as const
const STATUS_TONE = {
  added: 'text-good',
  deleted: 'text-bad',
  modified: 'text-warn',
  renamed: 'text-info',
  copied: 'text-info',
} as const

/** A changed file: how it changed, its name, and the lines - or, for a
 * binary, a git LFS file or a submodule, what it is and its size. */
function FileRow({ file }: { file: FileChange }) {
  const { t } = useTranslation()
  const name = file.path.split('/').at(-1) ?? file.path
  const stored = storedAs(file)
  const icon =
    stored?.kind === 'submodule' ? <Package aria-hidden /> : stored !== null ? <FileBox aria-hidden /> : file.lines !== null ? <FileCode aria-hidden /> : <File aria-hidden />
  return (
    <span
      className="furca-file-row flex min-w-0 flex-1 items-center gap-1.5"
      title={file.from === null ? file.path : `${file.from} → ${file.path}`}
    >
      <span className={cn('w-3 shrink-0 text-center font-mono text-2xs font-semibold', STATUS_TONE[file.status])} aria-label={t(`details.status.${file.status}`)}>
        {STATUS[file.status]}
      </span>
      <span className="flex shrink-0 text-dim [&>svg]:size-3.5">{icon}</span>
      <span className={cn('truncate', file.status === 'deleted' && 'text-dim line-through')}>{name}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1.5 pl-2 font-mono text-2xs">
        {stored !== null ? (
          <StoredBadge file={file} />
        ) : file.lines !== null ? (
          <>
            {file.lines.added > 0 && <span className="text-good">+{file.lines.added}</span>}
            {file.lines.removed > 0 && <span className="text-bad">−{file.lines.removed}</span>}
          </>
        ) : null}
      </span>
    </span>
  )
}

/** What a file that is not text is, and its size: a binary committed as it
 * is warns, a git LFS file does not. */
function StoredBadge({ file }: { file: FileChange }) {
  const { t } = useTranslation()
  const stored = storedAs(file)
  if (stored === null) return null
  const size = stored.after ?? stored.before
  return (
    <Badge variant={isRawBinary(file) ? 'warn' : stored.kind === 'lfs' ? 'info' : 'soft'} className="font-mono">
      {t(`details.stored.${stored.kind}`)}
      {size !== null && ` ${sizeText(size)}`}
    </Badge>
  )
}

/** The change of one file: what it is, then its lines. */
function DiffPane({
  path,
  commit,
  file,
  layout,
  onLayout,
}: {
  path: string
  commit: string
  file: FileChange
  layout: DiffLayout
  onLayout: (layout: DiffLayout) => void
}) {
  const { t } = useTranslation()
  const shown = useFileDiff(path, commit, file)
  useEffect(() => {
    if (file.lines !== null) prepareLanguage(file.path)
  }, [file])
  const endings = endingsChange(file)
  const modeChanged = file.before !== null && file.after !== null && file.before.mode !== file.after.mode

  let body: ReactNode
  if (shown === null || shown.read.state === 'reading') {
    body = (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('details.readingDiff')} />
      </div>
    )
  } else if (shown.read.state === 'failed') {
    body = (
      <div className="p-3">
        <Alert tone="bad" title={t('details.diffFailed')}>
          {shown.read.reason}
        </Alert>
      </div>
    )
  } else if (shown.read.value.file.lines !== null) {
    body = (
      <ErrorBoundary
        resetKey={shown.read.value}
        labels={{
          title: t('details.drawFailed'),
          body: t('details.drawFailedBody'),
          details: t('details.drawFailedDetails'),
          retry: t('details.drawAgain'),
        }}
      >
        <Suspense
          fallback={
            <div className="flex h-full items-center justify-center">
              <Spinner label={t('details.readingDiff')} />
            </div>
          }
        >
          <CodeDiff diff={shown.read.value} layout={layout} onShown={shown.stale ? undefined : markShown} />
        </Suspense>
      </ErrorBoundary>
    )
  } else {
    body = <StoredNote diff={shown.read.value} />
  }

  return (
    <section className="flex h-full min-h-0 flex-col" aria-label={t('details.diffLabel', { path: file.path })}>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-line px-3 text-2xs">
        <span className="flex min-w-0 items-center gap-1 font-mono text-xs">
          {file.from !== null && (
            <>
              <span className="truncate text-dim">{file.from}</span>
              <ArrowRight aria-hidden className="size-3 shrink-0 text-faint" />
            </>
          )}
          <span className="truncate">{file.path}</span>
        </span>
        {file.similarity !== null && file.similarity < 100 && (
          <span className="shrink-0 text-dim">{t('details.similar', { percent: file.similarity })}</span>
        )}
        {file.similarity === 100 && <span className="shrink-0 text-dim">{t('details.unchangedContent')}</span>}
        {modeChanged && (
          <Badge variant="soft">{t('details.mode', { before: t(`details.modes.${file.before!.mode}`), after: t(`details.modes.${file.after!.mode}`) })}</Badge>
        )}
        {endings !== null && <Badge variant="warn">{t('details.endings', endings)}</Badge>}
        <SegmentedControl
          value={layout}
          onValueChange={(value) => onLayout(value as DiffLayout)}
          className="ml-auto shrink-0"
        >
          <Segment value="split">{t('details.split')}</Segment>
          <Segment value="inline">{t('details.inline')}</Segment>
        </SegmentedControl>
      </div>
      <div className="min-h-0 flex-1">{body}</div>
    </section>
  )
}

/** A change with no lines to show: what each side is, and its size. */
function StoredNote({ diff }: { diff: FileDiff }) {
  const { t } = useTranslation()
  const { file } = diff
  const stored = storedAs(file)
  const side = (which: 'before' | 'after') => {
    const value = file[which]
    if (value === null) return t('details.absent')
    if (value.content.kind === 'submodule') return t('details.atCommit', { id: short(value.id) })
    if (value.content.kind === 'lfs') return t('details.lfsSide', { size: sizeText(value.size ?? 0), oid: value.content.oid.slice(0, 12) })
    if (value.content.kind === 'binary') return t('details.binarySide', { size: sizeText(value.size ?? 0) })
    return t('details.textSide', { size: sizeText(value.size ?? 0) })
  }
  const title =
    stored?.kind === 'submodule'
      ? t('details.submoduleTitle')
      : stored?.kind === 'lfs'
        ? t('details.lfsTitle')
        : t('details.binaryTitle')
  const description =
    stored?.kind === 'submodule'
      ? t('details.submoduleBody')
      : stored?.kind === 'lfs'
        ? t('details.lfsBody')
        : t('details.binaryBody')
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
      <EmptyState plain title={title} body={description} />
      <dl className="grid grid-cols-[auto_auto] gap-x-4 gap-y-1 text-left text-xs">
        <dt className="text-dim">{t('details.before')}</dt>
        <dd className="font-mono">{side('before')}</dd>
        <dt className="text-dim">{t('details.after')}</dt>
        <dd className="font-mono">{side('after')}</dd>
      </dl>
    </div>
  )
}
