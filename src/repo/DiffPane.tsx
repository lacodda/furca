import { useEffect, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowRight, X } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { EmptyState } from '@/components/ui/empty-state'
import { ErrorBoundary } from '@/components/ui/error-boundary'
import { Segment, SegmentedControl } from '@/components/ui/segmented-control'
import { Spinner } from '@/components/ui/spinner'
import { drawsTheSame, endingsChange, sizeText, storedAs } from '@/lib/changes'
import { markShown, useFileDiff } from '@/lib/details'
import type { DiffLayout } from '@/lib/generated/DiffLayout'
import type { FileChange } from '@/lib/generated/FileChange'
import type { FileDiff } from '@/lib/generated/FileDiff'
import type { Hunk } from '@/lib/generated/Hunk'
import type { Picked } from '@/lib/generated/Picked'
import { short } from '@/lib/names'
import { NO_PICKS, pickCount, pickedOf, stillPicked, wholeHunk, type Picks } from '@/lib/picks'
import type { HunkAct } from './CodeDiff'
import { ConfirmDiscard } from './ConfirmDiscard'

type CodeDiffModule = typeof import('./CodeDiff')

/** The drawing of a change, once it has been fetched. */
let loaded: CodeDiffModule | null = null

/** CodeMirror, fetched when the first diff is opened; `preloadDiff` fetches
 * it sooner, once the first frame is out of the way. */
const loadCodeDiff = () =>
  import('./CodeDiff').then((module) => {
    loaded = module
    return module
  })

export function preloadDiff(): void {
  void loadCodeDiff()
}

/**
 * The drawing of a change, or `null` until it is here. Not `lazy` and
 * `Suspense`: React holds back what a boundary reveals for 300 ms after it
 * drew the fallback, so that spinners do not flash - and a first diff worth
 * 90 ms of work took 400. A failed fetch is thrown for the error boundary
 * around the drawing, as `lazy` would.
 */
function useCodeDiff(): CodeDiffModule | null {
  const [module, setModule] = useState(loaded)
  const [failed, setFailed] = useState<{ cause: unknown } | null>(null)
  useEffect(() => {
    if (module !== null) return
    let current = true
    loadCodeDiff().then(
      (fetched) => {
        if (current) setModule(fetched)
      },
      (cause: unknown) => {
        if (current) setFailed({ cause })
      },
    )
    return () => {
      current = false
    }
  }, [module])
  if (failed !== null) throw failed.cause
  return module
}

/** Starts loading the language of the file at `path` beside the read of
 * its change. */
function prepareLanguage(path: string): void {
  void loadCodeDiff().then((module) => module.prepare(path))
}

/** What can be done to the lines of a change in the working copy: move them
 * across - into the index, or out of it - and, when they are not staged,
 * discard them. Hunks are taken whole from their bars, and picked lines from
 * the header. */
export interface LineActions {
  /** The lines are staged, and moving them takes them out of the index. */
  staged: boolean
  move: (picks: Picked[]) => void
  discard?: (picks: Picked[]) => void
}

/** The change of one file: what it is, then its lines. `scope` says what the
 * change is read under - a commit, or a moment of the working copy - so the
 * same file is read again once it means something else. */
export function DiffPane({
  path,
  scope,
  file,
  layout,
  onLayout,
  lines,
}: {
  path: string
  scope: string
  file: FileChange
  layout: DiffLayout
  onLayout: (layout: DiffLayout) => void
  lines?: LineActions
}) {
  const { t } = useTranslation()
  const shown = useFileDiff(path, scope, file)
  const [picks, setPicks] = useState<Picks>(NO_PICKS)
  // Lines are picked in the change of one file: another file starts with none,
  // and a pick the file read again no longer changes is let go.
  const [pickedIn, setPickedIn] = useState(file.path)
  const read = shown?.read.state === 'read' && shown.read.value.file.path === file.path ? shown.read.value : null
  let current = picks
  if (pickedIn !== file.path) {
    current = NO_PICKS
  } else if (read !== null) {
    current = stillPicked(picks, read.hunks)
  }
  if (current !== picks || pickedIn !== file.path) {
    setPicks(current)
    setPickedIn(file.path)
  }
  const [asked, setAsked] = useState<{ picks: Picked[]; lines: number } | null>(null)
  const acting = lines !== undefined && read !== null && shown?.stale !== true && read.before !== null && read.after !== null
  const picked = pickCount(current)

  const move = (chosen: Picked[]) => {
    setPicks(NO_PICKS)
    lines?.move(chosen)
  }
  const discard = (chosen: Picked[]) => {
    const count = chosen.reduce((sum, each) => sum + each.removed.length + each.added.length, 0)
    setAsked({ picks: chosen, lines: count })
  }
  const hunkActs: HunkAct[] | undefined =
    lines === undefined
      ? undefined
      : [
          { label: lines.staged ? t('changes.unstageHunk') : t('changes.stageHunk'), act: (hunk) => move([wholeHunk(hunk)]) },
          ...(lines.discard === undefined
            ? []
            : [{ label: t('changes.discardHunk'), bad: true, act: (hunk: Hunk) => discard([wholeHunk(hunk)]) }]),
        ]
  useEffect(() => {
    prepareLanguage(file.path)
  }, [file.path])
  // A list of the working copy names a file before anything has read it, and
  // what was read says the rest.
  const known = shown?.read.state === 'read' && shown.read.value.file.path === file.path ? shown.read.value.file : file
  const endings = endingsChange(known)
  const modeChanged = known.before !== null && known.after !== null && known.before.mode !== known.after.mode

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
        <DrawnDiff
          diff={shown.read.value}
          layout={layout}
          onShown={shown.stale ? undefined : markShown}
          hunkActs={shown.stale ? undefined : hunkActs}
          picks={current}
          onPicks={acting ? setPicks : undefined}
        />
      </ErrorBoundary>
    )
  } else {
    body = <StoredNote diff={shown.read.value} />
  }

  return (
    <section className="flex h-full min-h-0 flex-col" aria-label={t('details.diffLabel', { path: file.path })}>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-line px-3 text-2xs">
        <span className="flex min-w-0 items-center gap-1 font-mono text-xs">
          {known.from !== null && (
            <>
              <span className="truncate text-dim">{known.from}</span>
              <ArrowRight aria-hidden className="size-3 shrink-0 text-faint" />
            </>
          )}
          <span className="truncate">{file.path}</span>
        </span>
        {known.similarity !== null && known.similarity < 100 && (
          <span className="shrink-0 text-dim">{t('details.similar', { percent: known.similarity })}</span>
        )}
        {known.similarity === 100 && <span className="shrink-0 text-dim">{t('details.unchangedContent')}</span>}
        {modeChanged && (
          <Badge variant="soft">{t('details.mode', { before: t(`details.modes.${known.before!.mode}`), after: t(`details.modes.${known.after!.mode}`) })}</Badge>
        )}
        {endings !== null && <Badge variant="warn">{t('details.endings', endings)}</Badge>}
        {acting && lines !== undefined && read !== null && (
          <div className="ml-auto flex min-w-0 shrink items-center gap-1.5">
            {picked === 0 ? (
              <span className="truncate text-faint">{t('changes.pickHint')}</span>
            ) : (
              <>
                <span className="shrink-0 text-dim">{t('changes.picked', { count: picked })}</span>
                <Button size="xs" variant="soft" onClick={() => move(pickedOf(current, read.hunks))}>
                  {lines.staged ? t('changes.unstageLines') : t('changes.stageLines')}
                </Button>
                {lines.discard !== undefined && (
                  <Button size="xs" variant="danger" onClick={() => discard(pickedOf(current, read.hunks))}>
                    {t('changes.discardLines')}
                  </Button>
                )}
                <Button
                  size="icon-xs"
                  variant="icon"
                  aria-label={t('changes.letGo')}
                  title={t('changes.letGo')}
                  onClick={() => setPicks(NO_PICKS)}
                >
                  <X aria-hidden />
                </Button>
              </>
            )}
          </div>
        )}
        <SegmentedControl
          value={layout}
          onValueChange={(value) => onLayout(value as DiffLayout)}
          className={acting ? 'shrink-0' : 'ml-auto shrink-0'}
        >
          <Segment value="split">{t('details.split')}</Segment>
          <Segment value="inline">{t('details.inline')}</Segment>
        </SegmentedControl>
      </div>
      <div className="min-h-0 flex-1">{body}</div>
      <ConfirmDiscard
        open={asked !== null}
        onOpenChange={(open) => {
          if (!open) setAsked(null)
        }}
        title={t('changes.discardLinesTitle', { count: asked?.lines ?? 0, path: file.path })}
        body={t('changes.discardBody')}
        confirm={t('changes.discardLines')}
        onConfirm={() => {
          if (asked === null) return
          setPicks(NO_PICKS)
          lines?.discard?.(asked.picks)
          setAsked(null)
        }}
      />
    </section>
  )
}

/** The lines of a change, drawn once the drawing has been fetched. */
function DrawnDiff({
  diff,
  layout,
  onShown,
  hunkActs,
  picks,
  onPicks,
}: {
  diff: FileDiff
  layout: DiffLayout
  onShown?: () => void
  hunkActs?: readonly HunkAct[]
  picks: Picks
  onPicks?: (picks: Picks) => void
}) {
  const { t } = useTranslation()
  const module = useCodeDiff()
  const [drawn, setDrawn] = useState(diff)
  if (!drawsTheSame(drawn, diff)) setDrawn(diff)
  if (module === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('details.readingDiff')} />
      </div>
    )
  }
  return <module.default diff={drawn} layout={layout} onShown={onShown} hunkActs={hunkActs} picks={picks} onPicks={onPicks} />
}

/** A change with no lines to show: what each side is, and its size. */
function StoredNote({ diff }: { diff: FileDiff }) {
  const { t } = useTranslation()
  const { file } = diff
  const stored = storedAs(file)
  const side = (which: 'before' | 'after') => {
    const value = file[which]
    if (value === null) return t('details.absent')
    if (value.content.kind === 'submodule') return value.id === null ? t('details.aRepository') : t('details.atCommit', { id: short(value.id) })
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
