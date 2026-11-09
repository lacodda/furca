import { useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ArrowDownToLine, ArrowUpFromLine, ChevronDown, RefreshCw, X } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
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
import { Field } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Menu, MenuItem, MenuPopup, MenuSeparator, MenuTrigger } from '@/components/ui/menu'
import { Spinner } from '@/components/ui/spinner'
import { api, failure } from '@/lib/api'
import type { Remote } from '@/lib/generated/Remote'
import type { Sidebar } from '@/lib/generated/Sidebar'
import { aheadBehind } from '@/lib/names'
import { dismiss, fetchFrom, pullInto, pushTo, removeRemote, useReach, type Doing } from '@/lib/remote'
import { useStanding } from '@/lib/repos'
import { ConfirmDiscard } from './ConfirmDiscard'

/*
 * The remotes from the window: Fetch, Pull and Push above the history, each
 * with a menu of the other ways to do it, and the dialogs that add, change and
 * remove a remote. Every one is a run of your own git (ADR 0021): its
 * credential helper and ssh-agent answer for it, and furca never asks for a
 * password.
 */

/** The three buttons, and what each one's menu offers besides. */
export function ReachButtons({ path, sidebar, labelClass }: { path: string; sidebar: Sidebar | null; labelClass?: string }) {
  const { t } = useTranslation()
  const reach = useReach(path)
  const read = useStanding(path)
  const [adding, setAdding] = useState(false)
  const [forcing, setForcing] = useState(false)
  const remotes = sidebar?.remotes ?? []
  const branch = sidebar?.head.branch ?? null
  const upstream = sidebar?.head.upstream ?? null
  const tracking = read.state === 'read' ? read.value.upstream : null
  const sync = tracking ? aheadBehind(tracking) : null
  const busy = reach.run !== null
  const many = remotes.length > 1
  const none = remotes.length === 0

  const going = (doing: Doing) => (reach.run?.doing === doing ? <Spinner size="sm" label={t(`reach.doing.${doing}`)} /> : null)

  return (
    <div className="flex shrink-0 items-center gap-1">
      <Split
        label={t('reach.fetch')}
        icon={going('fetch') ?? <RefreshCw aria-hidden />}
        title={none ? t('reach.noRemotes') : t('reach.fetchHelp')}
        disabled={busy || none}
        onClick={() => void fetchFrom(path, { to: 'all' })}
        labelClass={labelClass}
        more={t('reach.fetchMore')}
        moreDisabled={busy}
      >
        <MenuItem disabled={none} onClick={() => void fetchFrom(path, { to: 'all' }, true)}>
          {t('reach.fetchPrune')}
        </MenuItem>
        {many &&
          remotes.map((remote) => (
            <MenuItem key={remote.name} onClick={() => void fetchFrom(path, { to: 'named', name: remote.name })}>
              {t('reach.fetchOne', { remote: remote.name })}
            </MenuItem>
          ))}
        {!none && <MenuSeparator />}
        <MenuItem onClick={() => setAdding(true)}>{t('remote.add')}</MenuItem>
      </Split>
      <Split
        label={t('reach.pull')}
        icon={going('pull') ?? <ArrowDownToLine aria-hidden />}
        count={sync && sync.behind > 0 ? `↓${String(sync.behind)}` : undefined}
        title={branch === null ? t('reach.detached') : upstream === null ? t('reach.noUpstream', { branch }) : t('reach.pullHelp', { upstream })}
        disabled={busy || upstream === null}
        onClick={() => void pullInto(path, 'as-configured')}
        labelClass={labelClass}
        more={t('reach.pullMore')}
        moreDisabled={busy || upstream === null}
      >
        <MenuItem onClick={() => void pullInto(path, 'merge')}>{t('reach.pullMerge')}</MenuItem>
        <MenuItem onClick={() => void pullInto(path, 'rebase')}>{t('reach.pullRebase')}</MenuItem>
        <MenuItem onClick={() => void pullInto(path, 'fast-forward')}>{t('reach.pullFastForward')}</MenuItem>
      </Split>
      <Split
        label={t('reach.push')}
        icon={going('push') ?? <ArrowUpFromLine aria-hidden />}
        count={sync && sync.ahead > 0 ? `↑${String(sync.ahead)}` : undefined}
        title={
          branch === null
            ? t('reach.detached')
            : none
              ? t('reach.noRemotes')
              : upstream === null
                ? t('reach.pushNew', { branch })
                : t('reach.pushHelp', { branch })
        }
        disabled={busy || branch === null || none}
        onClick={() => void pushTo(path, { to: 'default' })}
        labelClass={labelClass}
        more={t('reach.pushMore')}
        moreDisabled={busy || branch === null || none}
      >
        {many && (
          <>
            <MenuItem onClick={() => void pushTo(path, { to: 'all' })}>{t('reach.pushAll')}</MenuItem>
            {remotes.map((remote) => (
              <MenuItem key={remote.name} onClick={() => void pushTo(path, { to: 'named', name: remote.name })}>
                {t('reach.pushOne', { remote: remote.name })}
              </MenuItem>
            ))}
            <MenuSeparator />
          </>
        )}
        <MenuItem tone="danger" onClick={() => setForcing(true)}>
          {t('reach.force')}
        </MenuItem>
      </Split>
      <RemoteDialog path={path} open={adding} onOpenChange={setAdding} />
      <ConfirmDiscard
        open={forcing}
        onOpenChange={setForcing}
        title={t('reach.forceTitle', { branch: branch ?? '' })}
        body={t('reach.forceBody', {
          ...whereItGoes(tracking?.name ?? null, remotes, branch ?? ''),
          lost: t('reach.lost', { count: sync?.behind ?? 0 }),
        })}
        confirm={t('reach.forceConfirm')}
        onConfirm={() => {
          setForcing(false)
          void pushTo(path, { to: 'default' }, true)
        }}
      />
    </div>
  )
}

/** The remote and the branch there a push goes to by default, as the window
 * can tell it: the branch the current one follows - `origin/main` is `main`
 * on `origin`, the longest remote name winning - or, for one that follows
 * nothing, its own name on the only remote or `origin`. */
function whereItGoes(tracking: string | null, remotes: Remote[], branch: string): { remote: string; branch: string } {
  const owner = tracking
    ? remotes
        .map((remote) => remote.name)
        .filter((name) => tracking.startsWith(`${name}/`))
        .sort((a, b) => b.length - a.length)[0]
    : undefined
  if (tracking && owner) return { remote: owner, branch: tracking.slice(owner.length + 1) }
  const only = remotes.length === 1 ? remotes[0]?.name : remotes.find((remote) => remote.name === 'origin')?.name
  return { remote: only ?? '', branch }
}

/** A button and, joined to it, the menu of what else it can do. */
function Split({
  label,
  icon,
  count,
  title,
  disabled,
  onClick,
  labelClass,
  more,
  moreDisabled,
  children,
}: {
  label: string
  /** Where the label goes when the bar is short: it stays a screen reader's. */
  labelClass?: string
  icon: ReactNode
  count?: string
  title: string
  disabled: boolean
  onClick: () => void
  /** What the menu's button is called for a screen reader. */
  more: string
  moreDisabled: boolean
  children: ReactNode
}) {
  return (
    <div className="flex items-center">
      <Button variant="ghost" size="sm" className="rounded-r-none pr-1.5" title={title} disabled={disabled} onClick={onClick}>
        {icon}
        <span className={labelClass}>{label}</span>
        {count !== undefined && <span className="font-mono text-2xs text-dim">{count}</span>}
      </Button>
      <Menu>
        <MenuTrigger
          render={<Button variant="ghost" size="icon-sm" className="w-5 rounded-l-none" aria-label={more} title={more} disabled={moreDisabled} />}
        >
          <ChevronDown aria-hidden />
        </MenuTrigger>
        <MenuPopup align="start" size="lg">
          {children}
        </MenuPopup>
      </Menu>
    </div>
  )
}

/** What went wrong with the last run on a remote, in git's words, until it is
 * put away or the next run starts. */
export function ReachFailure({ path }: { path: string }) {
  const { t } = useTranslation()
  const reach = useReach(path)
  const outcome = reach.outcome
  if (outcome === null || outcome.ok) return null
  const took = outcome.pushes?.pushed.map((pushed) => pushed.remote) ?? []
  return (
    <div className="shrink-0 border-b border-line bg-raise px-3 py-2">
      <Alert
        tone="bad"
        title={took.length > 0 ? t('reach.failedSome', { remotes: took.join(', ') }) : t(`reach.failed.${outcome.doing}`)}
      >
        <div className="flex items-start gap-2">
          <p className="max-h-32 min-w-0 flex-1 overflow-y-auto font-mono text-2xs whitespace-pre-wrap">{outcome.reason}</p>
          <Button variant="icon" size="icon-xs" aria-label={t('reach.dismiss')} title={t('reach.dismiss')} onClick={() => dismiss(path)}>
            <X aria-hidden />
          </Button>
        </div>
      </Alert>
    </div>
  )
}

/**
 * Adds a remote - and fetches it, since a remote without its branches shows
 * nothing - or, given `remote`, renames it and points it elsewhere. The
 * address is shown without the password git may keep in it; left as it is, it
 * is left as git has it.
 */
export function RemoteDialog({
  path,
  remote,
  open,
  onOpenChange,
}: {
  path: string
  remote?: Remote
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup size="sm">
        {/* Drawn only while the dialog is open, so each opening starts from
            the remote as it is now. */}
        <RemoteForm path={path} remote={remote} onDone={() => onOpenChange(false)} />
      </DialogPopup>
    </Dialog>
  )
}

function RemoteForm({ path, remote, onDone }: { path: string; remote?: Remote; onDone: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(remote?.name ?? '')
  const [url, setUrl] = useState(remote?.url ?? '')
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)

  function submit(event: FormEvent) {
    event.preventDefault()
    const [named, at] = [name.trim(), url.trim()]
    setBusy(true)
    setSaid(null)
    const done = remote ? api.remoteEdit(path, remote.name, named, at) : api.remoteAdd(path, named, at)
    done.then(
      () => {
        setBusy(false)
        onDone()
        if (!remote) void fetchFrom(path, { to: 'named', name: named })
      },
      (cause: unknown) => {
        setBusy(false)
        setSaid(failure(cause))
      },
    )
  }

  return (
    <form onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>{remote ? t('remote.editTitle', { name: remote.name }) : t('remote.addTitle')}</DialogTitle>
        <DialogDescription>{remote ? t('remote.editBody') : t('remote.addBody')}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <Field label={t('remote.name')}>
          <Input autoFocus value={name} spellCheck={false} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Field label={t('remote.url')} help={t('remote.urlHelp')}>
          <Input value={url} spellCheck={false} onChange={(event) => setUrl(event.target.value)} />
        </Field>
        {said !== null && (
          <Alert tone="bad" title={t('remote.failed')}>
            <span className="whitespace-pre-wrap">{said}</span>
          </Alert>
        )}
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('remote.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || name.trim() === '' || url.trim() === ''}>
          {remote ? t('remote.save') : t('remote.addAndFetch')}
        </Button>
      </DialogActions>
    </form>
  )
}

/** The question before a remote is removed from the repository. */
export function RemoveRemote({
  path,
  name,
  open,
  onOpenChange,
}: {
  path: string
  name: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  return (
    <ConfirmDiscard
      open={open}
      onOpenChange={onOpenChange}
      title={t('remote.removeTitle', { name })}
      body={t('remote.removeBody', { name })}
      confirm={t('remote.remove')}
      onConfirm={() => {
        onOpenChange(false)
        void removeRemote(path, name)
      }}
    />
  )
}
