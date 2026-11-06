import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { cn } from 'dowel-ui'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Copyable } from '@/components/ui/copyable'
import {
  Dialog,
  DialogActions,
  DialogBody,
  DialogClose,
  DialogDescription,
  DialogHeader,
  DialogPopup,
  DialogTitle,
} from '@/components/ui/dialog'
import { EmptyState } from '@/components/ui/empty-state'
import { RowButton } from '@/components/ui/list-row'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Spinner } from '@/components/ui/spinner'
import { Splitter, SplitterHandle, SplitterPane } from '@/components/ui/splitter'
import { StatusDot, type Status as Dot } from '@/components/ui/status-dot'
import { windowFor } from '@/components/ui/virtual-list'
import type { Check } from '@/lib/generated/Check'
import type { CheckStatus } from '@/lib/generated/CheckStatus'
import type { Layout } from '@/lib/generated/Layout'
import type { Plan } from '@/lib/generated/Plan'
import type { ReleaseState } from '@/lib/generated/ReleaseState'
import {
  allowed,
  atTheEnd,
  beginning,
  chosenOrShown,
  linesOfStep,
  readPlan,
  startRun,
  useRelease,
  type Chosen,
  type SeenStep,
} from '@/lib/release'
import { useScrollBox } from './useScrollBox'

const CHECK_DOT: Record<CheckStatus, Dot> = { pass: 'good', warn: 'warn', fail: 'bad', skip: 'neutral' }
const STEP_DOT: Record<Exclude<SeenStep['status'], 'running'>, Dot> = { done: 'good', skipped: 'neutral', stopped: 'bad', ahead: 'neutral' }

export interface ReleasePanelProps {
  path: string
  layout: Layout
  onLayout: (layout: Layout) => void
}

/**
 * The release of a repository, as `furca release` has it in a terminal: the plan
 * and what stands in its way, the steps of the run as they go with what each
 * prints, and plan, run and resume. Red stops it here as it does there, because
 * it is the engine that says so: the panel only draws what it hears.
 */
export function ReleasePanel({ path, layout, onLayout }: ReleasePanelProps) {
  const { t } = useTranslation()
  const { state } = useRelease(path)

  if (state === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('release.reading')} />
      </div>
    )
  }
  if (!state.configured) {
    return <EmptyState plain title={t('release.none')} body={t('release.noneBody')} className="h-full" />
  }
  return <Opened key={path} path={path} state={state} layout={layout} onLayout={onLayout} />
}

function Opened({ path, state, layout, onLayout }: ReleasePanelProps & { state: ReleaseState }) {
  const { t } = useTranslation()
  const { plan: read, seen, running } = useRelease(path)
  const [picked, setPicked] = useState<Chosen | null>(null)
  const [confirming, setConfirming] = useState<'run' | 'resume' | null>(null)
  const list = useRef<HTMLDivElement>(null)

  const asked = read.state !== 'none'
  useEffect(() => {
    if (!asked) void readPlan(path)
  }, [asked, path])

  const plan = read.state === 'read' ? read.plan : read.state === 'reading' ? read.last : null
  const can = allowed(state, plan, running)
  const steps = seen?.steps ?? beginning(state.steps).steps
  const checks = plan?.checks ?? []
  const shown = chosenOrShown(picked, seen, checks)
  const failing = checks.filter((check) => check.status === 'fail').map((check) => check.name)
  const tag = state.under_way?.tag ?? seen?.report?.tag ?? plan?.proposed?.tag ?? ''

  // The steps stand below the checks, and a run is what is worth watching: the
  // row it moved to is brought into view, until the reader chooses one.
  const following = picked === null ? `${shown?.kind ?? ''}:${shown?.name ?? ''}` : null
  useEffect(() => {
    if (following !== null) list.current?.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest' })
  }, [following])

  const start = (resume: boolean) => {
    setConfirming(null)
    setPicked(null)
    void startRun(path, resume, state.steps)
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-3 border-b border-line bg-raise px-3 py-2">
        <Heading plan={plan} />
        <div className="flex shrink-0 items-center gap-1.5">
          <Button size="sm" disabled={running || read.state === 'reading'} onClick={() => void readPlan(path)}>
            {t('release.plan')}
          </Button>
          <Button size="sm" variant={can.run ? 'primary' : 'ghost'} disabled={!can.run} onClick={() => setConfirming('run')}>
            {t('release.run')}
          </Button>
          <Button size="sm" variant={can.resume ? 'primary' : 'ghost'} disabled={!can.resume} onClick={() => setConfirming('resume')}>
            {t('release.resume')}
          </Button>
        </div>
      </header>

      <Word
        state={state}
        seen={seen}
        tag={tag}
        running={running}
        failing={failing}
        reason={read.state === 'failed' ? read.reason : null}
      />

      <Splitter
        orientation="horizontal"
        defaultSizes={layout.files}
        onSizesChange={(sizes) => onLayout({ ...layout, files: sizes })}
        className="min-h-0 flex-1"
      >
        <SplitterPane min={15} max={60}>
          <ScrollArea label={t('release.listLabel')} orientation="vertical" className="h-full">
            <div ref={list} className="flex flex-col gap-0.5 p-1.5">
              {read.state === 'reading' && checks.length === 0 && (
                <div className="flex items-center justify-center p-3">
                  <Spinner label={t('release.reading')} />
                </div>
              )}
              {checks.length > 0 && <Section title={t('release.checks')} />}
              {checks.map((check) => (
                <RowButton
                  key={check.name}
                  selected={shown?.kind === 'check' && shown.name === check.name}
                  start={<StatusDot status={CHECK_DOT[check.status]} label={t(`release.status.${check.status}`)} />}
                  description={check.summary}
                  onClick={() => setPicked({ kind: 'check', name: check.name })}
                >
                  {check.name}
                </RowButton>
              ))}
              <Section title={t('release.steps')} />
              {steps.map((step) => (
                <RowButton
                  key={step.name}
                  selected={shown?.kind === 'step' && shown.name === step.name}
                  start={
                    step.status === 'running' ? (
                      <Spinner size="sm" label={t('release.status.running')} />
                    ) : (
                      <StatusDot status={STEP_DOT[step.status]} label={t(`release.status.${step.status}`)} />
                    )
                  }
                  description={step.summary === '' ? undefined : step.summary}
                  onClick={() => setPicked({ kind: 'step', name: step.name })}
                >
                  {step.name}
                </RowButton>
              ))}
            </div>
          </ScrollArea>
        </SplitterPane>
        <SplitterHandle label={t('details.resizeFiles')} />
        <SplitterPane min={30}>
          {shown?.kind === 'check' ? (
            <CheckPane check={checks.find((check) => check.name === shown.name)} />
          ) : shown?.kind === 'step' ? (
            <StepPane step={steps.find((step) => step.name === shown.name)} plan={plan} />
          ) : null}
        </SplitterPane>
      </Splitter>

      <Dialog open={confirming !== null} onOpenChange={(open) => !open && setConfirming(null)}>
        <DialogPopup size="sm">
          <DialogHeader>
            <DialogTitle>{t(confirming === 'resume' ? 'release.confirmResume' : 'release.confirmRun', { tag })}</DialogTitle>
          </DialogHeader>
          <DialogBody>
            <DialogDescription>{t('release.confirmBody')}</DialogDescription>
          </DialogBody>
          <DialogActions>
            <DialogClose render={<Button variant="ghost" />}>{t('release.cancel')}</DialogClose>
            <Button variant="primary" onClick={() => start(confirming === 'resume')}>
              {t(confirming === 'resume' ? 'release.resume' : 'release.run')}
            </Button>
          </DialogActions>
        </DialogPopup>
      </Dialog>
    </div>
  )
}

/** The stage and the number: which version this is, from where. */
function Heading({ plan }: { plan: Plan | null }) {
  const { t } = useTranslation()
  if (plan === null) return <div className="min-w-0 flex-1" />
  const { proposed, stage, released, tally } = plan
  const from = released ?? t('release.nothingReleased')
  return (
    <div className="min-w-0 flex-1">
      <h2 className="truncate text-sm font-medium">
        {proposed === null ? t('release.nothingToPropose') : proposed.tag}
        {stage !== null && <span className="font-normal text-dim"> · {stage.title}</span>}
      </h2>
      <p className="truncate text-2xs text-dim">
        {proposed !== null &&
          (proposed.bump === null
            ? t('release.line', { released: from, version: proposed.version })
            : t('release.lineStep', { released: from, version: proposed.version, bump: proposed.bump }))}
        {proposed !== null && ' · '}
        {t('release.commits', { count: tally.total })}
      </p>
    </div>
  )
}

/** The one thing to say about where the release stands: red stops it. */
function Word({
  state,
  seen,
  tag,
  running,
  failing,
  reason,
}: {
  state: ReleaseState
  seen: ReturnType<typeof useRelease>['seen']
  tag: string
  running: boolean
  failing: readonly string[]
  reason: string | null
}) {
  const { t } = useTranslation()
  const shell = 'm-2 mb-0 shrink-0'
  if (state.broken !== null) {
    return (
      <Alert tone="bad" title={t('release.broken')} className={shell}>
        {state.broken}
      </Alert>
    )
  }
  if (running) return null
  if (seen?.failed != null) {
    return (
      <Alert tone="bad" title={t('release.failed')} className={shell}>
        {seen.failed}
      </Alert>
    )
  }
  const stopped = seen?.steps.find((step) => step.status === 'stopped')
  if (stopped !== undefined) {
    return (
      <Alert tone="bad" title={t('release.stopped', { tag, step: stopped.name })} className={shell}>
        {stopped.summary}
      </Alert>
    )
  }
  if (seen?.report?.released === true) {
    return <Alert tone="good" title={t('release.out', { tag: seen.report.tag ?? tag })} className={shell} />
  }
  if (state.under_way?.resumable === true) {
    return (
      <Alert tone="warn" title={t('release.underWay', { tag: state.under_way.tag })} className={shell}>
        {t('release.underWayBody', { next: state.under_way.next ?? '' })}
      </Alert>
    )
  }
  if (reason !== null) {
    return (
      <Alert tone="bad" title={t('release.planFailed')} className={shell}>
        {reason}
      </Alert>
    )
  }
  if (failing.length > 0) {
    return (
      <Alert tone="bad" title={t('release.notReady', { failing: failing.join(', ') })} className={shell}>
        {t('release.notReadyBody')}
      </Alert>
    )
  }
  return null
}

function Section({ title }: { title: string }) {
  return <h3 className="px-2.5 pb-0.5 pt-2 text-2xs font-medium uppercase tracking-wide text-dim first:pt-0.5">{title}</h3>
}

function Lines({ lines }: { lines: readonly string[] }) {
  return (
    <ul className="flex flex-col gap-0.5 font-mono text-xs text-dim">
      {lines.map((line, index) => (
        <li key={index} className="whitespace-pre-wrap break-words">
          {line}
        </li>
      ))}
    </ul>
  )
}

function CheckPane({ check }: { check: Check | undefined }) {
  const { t } = useTranslation()
  if (check === undefined) return null
  return (
    <ScrollArea label={t('release.detailsLabel')} orientation="vertical" className="h-full">
      <div className="flex flex-col gap-2 p-3">
        <p className="text-sm">{check.summary}</p>
        <Lines lines={check.details} />
      </div>
    </ScrollArea>
  )
}

/** What a step said of itself, what it printed, and before it has run, what the
 * plan says it would do. */
function StepPane({ step, plan }: { step: SeenStep | undefined; plan: Plan | null }) {
  const { t } = useTranslation()
  const would = useMemo(() => (plan === null || step === undefined ? [] : plan.would.filter((each) => each.step === step.name).map((each) => each.what)), [plan, step])
  if (step === undefined) return null
  const lines = linesOfStep(step)
  const said = step.summary !== '' || step.details.length > 0
  if (!said && lines.length === 0) {
    return (
      <ScrollArea label={t('release.detailsLabel')} orientation="vertical" className="h-full">
        <div className="flex flex-col gap-2 p-3">
          {would.length > 0 ? (
            <>
              <p className="text-sm">{t('release.would')}</p>
              <Lines lines={would} />
            </>
          ) : (
            <p className="text-sm text-dim">{t('release.nothingToSay')}</p>
          )}
        </div>
      </ScrollArea>
    )
  }
  return (
    <div className="flex h-full min-h-0 flex-col">
      {said && (
        <ScrollArea label={t('release.detailsLabel')} orientation="vertical" className={cn('max-h-40 shrink-0', lines.length > 0 && 'border-b border-line')}>
          <div className="flex flex-col gap-2 p-3">
            {step.summary !== '' && <p className="text-sm">{step.summary}</p>}
            {step.details.length > 0 && <Lines lines={step.details} />}
          </div>
        </ScrollArea>
      )}
      {lines.length > 0 && <Log key={step.name} lines={lines} />}
    </div>
  )
}

/** How tall a line of the log is. */
const LINE_HEIGHT = 18

/**
 * What a step printed. A gate prints tens of thousands of lines, so only a
 * screenful is drawn, and it follows the end until the reader scrolls away from
 * it, and again once they scroll back.
 */
function Log({ lines }: { lines: readonly string[] }) {
  const { t } = useTranslation()
  const { viewport, scrollTop, viewportHeight, scrollTo } = useScrollBox()
  const total = lines.length * LINE_HEIGHT
  const following = useRef(true)

  useLayoutEffect(() => {
    if (viewportHeight > 0) following.current = atTheEnd(scrollTop, viewportHeight, viewport.current?.scrollHeight ?? 0)
    // Only where the reader has been: what new lines do to the end is not them
    // scrolling away from it.
  }, [scrollTop, viewportHeight, viewport])
  useLayoutEffect(() => {
    if (following.current && viewport.current !== null) scrollTo(viewport.current.scrollHeight)
  }, [lines.length, scrollTo, viewport, viewportHeight])

  const { start, end, offsetTop } = windowFor({ count: lines.length, rowHeight: LINE_HEIGHT, viewportHeight, scrollTop, overscan: 8 })

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-end border-b border-line px-3 py-0.5">
        <Copyable value={lines.join('\n')} label={t('release.copyLog')} copiedLabel={t('release.copied')} className="text-2xs">
          {t('release.copy')}
        </Copyable>
      </div>
      <ScrollArea label={t('release.logLabel')} orientation="both" className="min-h-0 flex-1" viewportRef={viewport}>
        <div role="log" aria-label={t('release.logLabel')} className="w-max min-w-full px-3 font-mono text-xs" style={{ height: total }}>
          <div style={{ transform: `translateY(${String(offsetTop)}px)` }}>
            {lines.slice(start, end).map((line, offset) => (
              <div key={start + offset} className="whitespace-pre" style={{ height: LINE_HEIGHT, lineHeight: `${String(LINE_HEIGHT)}px` }}>
                {line === '' ? ' ' : line}
              </div>
            ))}
          </div>
        </div>
      </ScrollArea>
    </div>
  )
}
