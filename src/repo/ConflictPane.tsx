import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { TriangleAlert } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Spinner } from '@/components/ui/spinner'
import { api, failure } from '@/lib/api'
import type { Conflict } from '@/lib/generated/Conflict'
import type { Conflicted } from '@/lib/generated/Conflicted'
import type { Halfway } from '@/lib/generated/Halfway'
import type { Resolution } from '@/lib/generated/Resolution'
import type { Side } from '@/lib/generated/Side'
import { bringing, takenWhole, type Labels } from '@/lib/merge'
import { short } from '@/lib/names'
import { useHalfway } from '@/lib/repos'
import { useWorkingCopy } from '@/lib/working'
import { ConfirmDiscard } from './ConfirmDiscard'

type ResolverModule = typeof import('./Resolver')

/** The resolver, fetched with CodeMirror when the first conflict is opened. */
let loaded: ResolverModule | null = null
const loadResolver = () =>
  import('./Resolver').then((module) => {
    loaded = module
    return module
  })

/** The resolver once it is here; a failed fetch is thrown for the error
 * boundary around it. Not `lazy`, for the reason the diff is not. */
function useResolver(): ResolverModule | null {
  const [module, setModule] = useState(loaded)
  const [failed, setFailed] = useState<{ cause: unknown } | null>(null)
  useEffect(() => {
    if (module !== null) return
    let current = true
    loadResolver().then(
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

/** What the two sides are called, as a person knows them: the branch on our
 * side, and on theirs what is being merged, picked, reverted or replayed. In
 * a rebase our side is the new base the commits are replayed onto. */
function sideLabels(halfway: Halfway | null, t: ReturnType<typeof useTranslation>['t']): Labels {
  if (halfway === null) return { ours: 'HEAD', theirs: t('merge.otherSide') }
  const branch = halfway.branch ?? 'HEAD'
  const commit = halfway.commit === null ? null : `${short(halfway.commit.id)} ${halfway.commit.subject}`
  switch (halfway.operation) {
    case 'merge':
      return { ours: branch, theirs: bringing(halfway) ?? t('merge.otherSide') }
    case 'rebase':
      return { ours: t('merge.newBase', { branch }), theirs: commit ?? t('merge.otherSide') }
    case 'revert':
      return { ours: branch, theirs: halfway.commit === null ? t('merge.otherSide') : t('merge.revertOf', { at: short(halfway.commit.id) }) }
    default:
      return { ours: branch, theirs: commit ?? t('merge.otherSide') }
  }
}

/**
 * A file in conflict, on the right of the working copy: its three panes, or -
 * for a file one side deleted, or a binary - the choice of one side whole.
 * Saving writes the file and stages it, and it leaves the list (ADR 0023).
 */
export function ConflictPane({ path, conflicted }: { path: string; conflicted: Conflicted }) {
  const { t } = useTranslation()
  const file = conflicted.path
  const copy = useWorkingCopy(path)
  const halfway = useHalfway(path)
  const labels = sideLabels(halfway.state === 'read' ? halfway.value : null, t)
  const [read, setRead] = useState<{ file: string; conflict: Conflict } | { file: string; failed: string } | null>(null)
  const [saving, setSaving] = useState(false)
  const [refused, setRefused] = useState<{ file: string; said: string } | null>(null)
  const [forcing, setForcing] = useState(false)
  // Read again whenever the working copy is: the stages stay as they were
  // while the file is in conflict, but whether the file still holds markers
  // moves as it is edited elsewhere.
  const status = copy.state === 'read' ? copy.status : null

  useEffect(() => {
    let current = true
    api.conflict(path, file).then(
      (conflict) => {
        if (current) setRead({ file, conflict })
      },
      (cause: unknown) => {
        if (current) setRead({ file, failed: failure(cause) })
      },
    )
    return () => {
      current = false
    }
  }, [path, file, status])

  const resolve = (how: Resolution) => {
    setSaving(true)
    setRefused(null)
    api.resolve(path, file, how).then(
      () => setSaving(false),
      (cause: unknown) => {
        setSaving(false)
        setRefused({ file, said: failure(cause) })
      },
    )
  }

  const known = read !== null && read.file === file ? read : null
  if (known === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('merge.reading')} />
      </div>
    )
  }
  if ('failed' in known) {
    return (
      <div className="p-3">
        <Alert tone="bad" title={t('merge.readFailed')}>
          {known.failed}
        </Alert>
      </div>
    )
  }
  const conflict = known.conflict
  const said = refused !== null && refused.file === file ? refused.said : null
  const asIs = () => {
    if (conflict.marked !== null) setForcing(true)
    else resolve({ take: 'as-is', force: false })
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-3 py-1.5 text-xs">
        <TriangleAlert aria-hidden className="size-3.5 shrink-0 text-warn" />
        <span className="min-w-0 truncate font-mono" title={conflict.path}>
          {conflict.path}
        </span>
        <span className="shrink-0 text-dim">{t(`changes.conflict.${conflict.kind}`)}</span>
        <Button size="xs" variant="ghost" className="ml-auto" disabled={saving} title={t('merge.asIsHelp')} onClick={asIs}>
          {t('merge.asIs')}
        </Button>
      </div>
      {/* git writes markers only into a file both sides changed as text: a
          file one side deleted, or a binary, never had any. */}
      {conflict.marked === null && !takenWhole(conflict) && (
        <div className="shrink-0 border-b border-line px-3 py-2">
          <Alert tone="warn" title={t('merge.noMarkersTitle')}>
            {t('merge.noMarkersBody')}
          </Alert>
        </div>
      )}
      {said !== null && (
        <p role="alert" className="max-h-24 shrink-0 overflow-y-auto border-b border-line px-3 py-1 text-2xs whitespace-pre-wrap text-bad">
          {said}
        </p>
      )}
      <div className="min-h-0 flex-1">
        {takenWhole(conflict) ? (
          <Whole conflict={conflict} labels={labels} saving={saving} onResolve={resolve} />
        ) : (
          <Blocks
            key={`${conflict.path}|${conflict.base?.id ?? ''}|${conflict.ours?.id ?? ''}|${conflict.theirs?.id ?? ''}|${labels.ours}|${labels.theirs}`}
            conflict={conflict}
            labels={labels}
            saving={saving}
            onSave={(text) => resolve({ take: 'text', text })}
          />
        )}
      </div>
      <ConfirmDiscard
        open={forcing}
        onOpenChange={setForcing}
        title={t('merge.forceTitle', { path: conflict.path })}
        body={t('merge.forceBody', { line: conflict.marked ?? 0 })}
        confirm={t('merge.forceConfirm')}
        onConfirm={() => {
          setForcing(false)
          resolve({ take: 'as-is', force: true })
        }}
      />
    </div>
  )
}

function Blocks({ conflict, labels, saving, onSave }: { conflict: Conflict; labels: Labels; saving: boolean; onSave: (text: string) => void }) {
  const { t } = useTranslation()
  const module = useResolver()
  if (module === null) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('merge.reading')} />
      </div>
    )
  }
  const Resolver = module.default
  return <Resolver conflict={conflict} labels={labels} saving={saving} onSave={onSave} />
}

/** A side as one line: what it holds, or that it deleted the file. */
function sideWords(side: Side | null, t: ReturnType<typeof useTranslation>['t']): string {
  if (side === null) return t('merge.sideDeleted')
  switch (side.content.kind) {
    case 'binary':
      return t('merge.sideBinary')
    case 'lfs':
      return t('merge.sideLfs')
    case 'submodule':
      return t('merge.sideSubmodule')
    default:
      return t('merge.sideText')
  }
}

/** The choice of one side whole, for a file one side deleted or that is not
 * text: what each side has, and a button for each. */
function Whole({
  conflict,
  labels,
  saving,
  onResolve,
}: {
  conflict: Conflict
  labels: Labels
  saving: boolean
  onResolve: (how: Resolution) => void
}) {
  const { t } = useTranslation()
  const sides = [
    { name: t('merge.oursPane', { name: labels.ours }), side: conflict.ours, take: { take: 'ours' } as Resolution },
    { name: t('merge.theirsPane', { name: labels.theirs }), side: conflict.theirs, take: { take: 'theirs' } as Resolution },
  ]
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-6 text-center">
      <p className="max-w-md text-sm text-dim">{t(`merge.whole.${conflict.kind}`)}</p>
      <div className="flex flex-wrap justify-center gap-3">
        {sides.map(({ name, side, take }) => (
          <div key={name} className="flex w-56 flex-col items-center gap-2 rounded-md border border-line bg-raise p-3">
            <span className="max-w-full truncate text-xs font-semibold" title={name}>
              {name}
            </span>
            <span className="text-xs text-dim">{sideWords(side, t)}</span>
            <Button size="xs" variant="primary" disabled={saving} onClick={() => onResolve(take)}>
              {side === null ? t('merge.takeDeleted') : t('merge.takeWhole')}
            </Button>
          </div>
        ))}
      </div>
    </div>
  )
}
