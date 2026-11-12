import { useEffect, useState, type FormEvent, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Alert } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  ContextMenu,
  ContextMenuItem,
  ContextMenuPopup,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import {
  ConfirmDialog,
  ConfirmDialogActions,
  ConfirmDialogBody,
  ConfirmDialogClose,
  ConfirmDialogDescription,
  ConfirmDialogHeader,
  ConfirmDialogPopup,
  ConfirmDialogTitle,
} from '@/components/ui/confirm-dialog'
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
import { Select, SelectItem, SelectPopup, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Textarea } from '@/components/ui/textarea'
import { api, failure } from '@/lib/api'
import type { Branch } from '@/lib/generated/Branch'
import type { Sidebar } from '@/lib/generated/Sidebar'
import { short } from '@/lib/names'
import { checkoutBranch, checkoutCommit, deleteBranch, deleteTag, pushTag, useReach } from '@/lib/remote'
import { ConfirmDiscard } from './ConfirmDiscard'
import { RemoteDialog, RemoveRemote } from './Reach'

/*
 * Branches and tags from the window: the dialogs that make, rename and delete
 * them and set what a branch follows, opened from the left column's menu and
 * from the graph's. What each one means - which new branch follows what, what
 * a deletion loses - is furca-core's (ADR 0022); the dialogs say it before it
 * is done, and git's own words when it refuses.
 */

/** A question the window is asking about a ref: which dialog is open. */
export type Asking =
  /** `label` is how `from` is shown - a branch's or a tag's name, a short
   * id - and `follows` the name that would follow it, for a remote branch. */
  | { kind: 'branch'; from: string; label: string; at: string; follows?: string }
  | { kind: 'rename'; branch: string }
  | { kind: 'follow'; branch: Branch }
  | { kind: 'delete-branch'; branch: Branch }
  | { kind: 'tag'; target: string; at: string }
  | { kind: 'delete-tag'; tag: string }
  | { kind: 'add-remote' }
  | { kind: 'edit-remote'; remote: string }
  | { kind: 'remove-remote'; remote: string }

/** The dialog `asking` names, over the repository at `path`. */
export function RefDialogs({
  path,
  sidebar,
  asking,
  onDone,
}: {
  path: string
  sidebar: Sidebar
  asking: Asking | null
  onDone: () => void
}) {
  const shut = (open: boolean) => {
    if (!open) onDone()
  }
  const kind = asking?.kind
  const remotes = sidebar.remotes.map((remote) => remote.name)
  return (
    <>
      <FormDialog open={kind === 'branch'} onOpenChange={shut}>
        {asking?.kind === 'branch' && <BranchForm path={path} asking={asking} onDone={onDone} />}
      </FormDialog>
      <FormDialog open={kind === 'rename'} onOpenChange={shut}>
        {asking?.kind === 'rename' && <RenameForm path={path} branch={asking.branch} onDone={onDone} />}
      </FormDialog>
      <FormDialog open={kind === 'follow'} onOpenChange={shut}>
        {asking?.kind === 'follow' && <FollowForm path={path} sidebar={sidebar} branch={asking.branch} onDone={onDone} />}
      </FormDialog>
      <DeleteBranch
        path={path}
        branch={asking?.kind === 'delete-branch' ? asking.branch : null}
        onOpenChange={shut}
      />
      <FormDialog open={kind === 'tag'} onOpenChange={shut}>
        {asking?.kind === 'tag' && <TagForm path={path} asking={asking} remotes={remotes} onDone={onDone} />}
      </FormDialog>
      {/* Drawn afresh for each tag, so no box ticked for the last one stays
          ticked. */}
      <DeleteTag
        key={asking?.kind === 'delete-tag' ? asking.tag : ''}
        path={path}
        tag={asking?.kind === 'delete-tag' ? asking.tag : null}
        remotes={remotes}
        onOpenChange={shut}
      />
      <RemoteDialog path={path} open={kind === 'add-remote'} onOpenChange={shut} />
      <RemoteDialog
        path={path}
        remote={asking?.kind === 'edit-remote' ? sidebar.remotes.find((remote) => remote.name === asking.remote) : undefined}
        open={kind === 'edit-remote'}
        onOpenChange={shut}
      />
      <RemoveRemote
        path={path}
        name={asking?.kind === 'remove-remote' ? asking.remote : ''}
        open={kind === 'remove-remote'}
        onOpenChange={shut}
      />
    </>
  )
}

/** A dialog drawn only while it is open, so each opening starts afresh. */
function FormDialog({
  open,
  onOpenChange,
  children,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  children: ReactNode
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogPopup size="sm">{children}</DialogPopup>
    </Dialog>
  )
}

/** How a form that runs git stands: going, and what git said if it refused. */
function useRun() {
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<string | null>(null)
  const run = (work: Promise<unknown>, done: () => void) => {
    setBusy(true)
    setSaid(null)
    work.then(
      () => {
        setBusy(false)
        done()
      },
      (cause: unknown) => {
        setBusy(false)
        setSaid(failure(cause))
      },
    )
  }
  return { busy, said, run }
}

function Refused({ said }: { said: string | null }) {
  const { t } = useTranslation()
  if (said === null) return null
  return (
    <Alert tone="bad" title={t('refs.failed')}>
      <span className="whitespace-pre-wrap">{said}</span>
    </Alert>
  )
}

/** A new branch at `from`, checked out unless the box is cleared. */
function BranchForm({ path, asking, onDone }: { path: string; asking: Asking & { kind: 'branch' }; onDone: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [checkout, setCheckout] = useState(true)
  const { busy, said, run } = useRun()

  function submit(event: FormEvent) {
    event.preventDefault()
    run(api.branchCreate(path, name.trim(), asking.from, checkout), onDone)
  }

  return (
    <form onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>{t('refs.branchTitle')}</DialogTitle>
        <DialogDescription>
          {asking.label === asking.at
            ? t('refs.branchAt', { at: asking.at })
            : t('refs.branchFrom', { from: asking.label, at: asking.at })}{' '}
          {asking.follows === undefined
            ? t('refs.branchAlone')
            : t('refs.branchFollows', { name: asking.follows, from: asking.label })}
        </DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <Field label={t('refs.name')} help={t('refs.nameHelp')}>
          <Input autoFocus value={name} spellCheck={false} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Checkbox checked={checkout} onCheckedChange={setCheckout}>
          {t('refs.checkoutAfter')}
        </Checkbox>
        <Refused said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || name.trim() === ''}>
          {checkout ? t('refs.createAndCheckout') : t('refs.create')}
        </Button>
      </DialogActions>
    </form>
  )
}

function RenameForm({ path, branch, onDone }: { path: string; branch: string; onDone: () => void }) {
  const { t } = useTranslation()
  const [name, setName] = useState(branch)
  const { busy, said, run } = useRun()

  function submit(event: FormEvent) {
    event.preventDefault()
    run(api.branchRename(path, branch, name.trim()), onDone)
  }

  return (
    <form onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>{t('refs.renameTitle', { name: branch })}</DialogTitle>
        <DialogDescription>{t('refs.renameBody')}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <Field label={t('refs.name')} help={t('refs.nameHelp')}>
          <Input autoFocus value={name} spellCheck={false} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Refused said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || name.trim() === '' || name.trim() === branch}>
          {t('refs.rename')}
        </Button>
      </DialogActions>
    </form>
  )
}

/** Which remote branch `branch` follows. It starts on the one it follows, or
 * else the one of its own name, or else the first there is. */
function FollowForm({
  path,
  sidebar,
  branch,
  onDone,
}: {
  path: string
  sidebar: Sidebar
  branch: Branch
  onDone: () => void
}) {
  const { t } = useTranslation()
  const names = sidebar.refs.remote_branches.map((each) => each.name)
  const ownName = names.find((name) => name.endsWith(`/${branch.name}`))
  const [chosen, setChosen] = useState<string>(branch.upstream?.name ?? ownName ?? names[0] ?? '')
  const { busy, said, run } = useRun()

  function submit(event: FormEvent) {
    event.preventDefault()
    run(api.branchFollow(path, branch.name, chosen), onDone)
  }

  return (
    <form onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>{t('refs.followTitle', { name: branch.name })}</DialogTitle>
        <DialogDescription>{names.length === 0 ? t('refs.noRemoteBranches') : t('refs.followBody')}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        {names.length > 0 && (
          <Field label={t('refs.followField')}>
            <Select value={chosen} onValueChange={(value) => setChosen(String(value))}>
              <SelectTrigger size="sm" aria-label={t('refs.followField')}>
                <SelectValue>{(value: unknown) => String(value)}</SelectValue>
              </SelectTrigger>
              <SelectPopup>
                {names.map((name) => (
                  <SelectItem key={name} value={name}>
                    {name}
                  </SelectItem>
                ))}
              </SelectPopup>
            </Select>
          </Field>
        )}
        <Refused said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || chosen === ''}>
          {t('refs.follow')}
        </Button>
      </DialogActions>
    </form>
  )
}

/** The question before a branch is deleted, with how many commits only it
 * holds: counted when the question opens, and said in the answer. */
function DeleteBranch({
  path,
  branch,
  onOpenChange,
}: {
  path: string
  branch: Branch | null
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const [lost, setLost] = useState<{ name: string; count: number } | { name: string; failed: string } | null>(null)
  const name = branch?.name ?? null

  useEffect(() => {
    if (name === null) return
    let current = true
    api.branchLost(path, name).then(
      (count) => {
        if (current) setLost({ name, count })
      },
      (cause: unknown) => {
        if (current) setLost({ name, failed: failure(cause) })
      },
    )
    return () => {
      current = false
    }
  }, [path, name])

  const known = lost !== null && lost.name === name ? lost : null
  const count = known !== null && 'count' in known ? known.count : null
  const body =
    known === null
      ? t('refs.deleteCounting')
      : 'failed' in known
        ? known.failed
        : t('refs.deleteBody', { count: known.count, at: branch ? short(branch.target) : '' })

  return (
    <ConfirmDialog open={branch !== null} onOpenChange={onOpenChange}>
      <ConfirmDialogPopup size="md">
        <ConfirmDialogHeader>
          <ConfirmDialogTitle className="break-words">{t('refs.deleteTitle', { name: name ?? '' })}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{body}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogActions>
          <ConfirmDialogClose render={<Button variant="ghost" />}>{t('changes.keep')}</ConfirmDialogClose>
          <Button
            variant="primary"
            className="bg-bad text-on-bad not-data-disabled:hover:bg-bad/85"
            disabled={count === null}
            onClick={() => {
              if (name === null || count === null) return
              onOpenChange(false)
              void deleteBranch(path, name, count)
            }}
          >
            {t('refs.deleteConfirm')}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}

/** A new tag on `target`: annotated when it has a message, and pushed once
 * it is made when the box is ticked. */
function TagForm({
  path,
  asking,
  remotes,
  onDone,
}: {
  path: string
  asking: Asking & { kind: 'tag' }
  remotes: string[]
  onDone: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')
  const [message, setMessage] = useState('')
  const [push, setPush] = useState(false)
  const { busy, said, run } = useRun()
  const only = remotes.length === 1 ? remotes[0] : undefined

  function submit(event: FormEvent) {
    event.preventDefault()
    const named = name.trim()
    run(api.tagCreate(path, named, asking.target, message.trim() === '' ? null : message), () => {
      onDone()
      if (push) void pushTag(path, named, only === undefined ? { to: 'all' } : { to: 'named', name: only })
    })
  }

  return (
    <form onSubmit={submit}>
      <DialogHeader>
        <DialogTitle>{t('refs.tagTitle')}</DialogTitle>
        <DialogDescription>{t('refs.tagBody', { at: asking.at })}</DialogDescription>
      </DialogHeader>
      <DialogBody className="flex flex-col gap-3">
        <Field label={t('refs.name')}>
          <Input autoFocus value={name} spellCheck={false} onChange={(event) => setName(event.target.value)} />
        </Field>
        <Field label={t('refs.tagMessage')} help={t('refs.tagMessageHelp')}>
          <Textarea rows={3} value={message} onChange={(event) => setMessage(event.target.value)} />
        </Field>
        {remotes.length > 0 && (
          <Checkbox checked={push} onCheckedChange={setPush}>
            {only === undefined ? t('refs.tagPushAll') : t('refs.tagPush', { remote: only })}
          </Checkbox>
        )}
        <Refused said={said} />
      </DialogBody>
      <DialogActions>
        <DialogClose render={<Button variant="ghost" />}>{t('refs.cancel')}</DialogClose>
        <Button type="submit" variant="primary" disabled={busy || name.trim() === ''}>
          {t('refs.tagCreate')}
        </Button>
      </DialogActions>
    </form>
  )
}

/** The question before a tag is deleted, with the remotes to take it away
 * from too: one that keeps it gives it back on the next fetch. */
function DeleteTag({
  path,
  tag,
  remotes,
  onOpenChange,
}: {
  path: string
  tag: string | null
  remotes: string[]
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation()
  const [there, setThere] = useState<string[]>([])
  if (remotes.length === 0) {
    return (
      <ConfirmDiscard
        open={tag !== null}
        onOpenChange={onOpenChange}
        title={t('refs.deleteTagTitle', { name: tag ?? '' })}
        body={t('refs.deleteTagHere')}
        confirm={t('refs.deleteConfirm')}
        onConfirm={() => {
          if (tag === null) return
          onOpenChange(false)
          void deleteTag(path, tag, [])
        }}
      />
    )
  }
  return (
    <ConfirmDialog open={tag !== null} onOpenChange={onOpenChange}>
      <ConfirmDialogPopup size="md">
        <ConfirmDialogHeader>
          <ConfirmDialogTitle className="break-words">{t('refs.deleteTagTitle', { name: tag ?? '' })}</ConfirmDialogTitle>
          <ConfirmDialogDescription>{t('refs.deleteTagBody')}</ConfirmDialogDescription>
        </ConfirmDialogHeader>
        <ConfirmDialogBody className="flex flex-col gap-2">
          {remotes.map((remote) => (
            <Checkbox
              key={remote}
              checked={there.includes(remote)}
              onCheckedChange={(checked) =>
                setThere((now) => (checked ? [...now, remote] : now.filter((each) => each !== remote)))
              }
            >
              {t('refs.deleteTagOn', { remote })}
            </Checkbox>
          ))}
        </ConfirmDialogBody>
        <ConfirmDialogActions>
          <ConfirmDialogClose render={<Button variant="ghost" />}>{t('changes.keep')}</ConfirmDialogClose>
          <Button
            variant="primary"
            className="bg-bad text-on-bad not-data-disabled:hover:bg-bad/85"
            onClick={() => {
              if (tag === null) return
              onOpenChange(false)
              void deleteTag(path, tag, remotes.filter((remote) => there.includes(remote)))
            }}
          >
            {t('refs.deleteConfirm')}
          </Button>
        </ConfirmDialogActions>
      </ConfirmDialogPopup>
    </ConfirmDialog>
  )
}

/** The commit a context menu over the graph was asked for: the row under the
 * pointer, or - from the keyboard, where the event comes from the grid - the
 * row chosen in it. */
function commitOf(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null
  const row = target.closest<HTMLElement>('[data-commit]')
  if (row) return row.dataset.commit ?? null
  const grid = target.closest('[role=grid]')
  const active = grid?.getAttribute('aria-activedescendant')
  const chosen = active ? document.getElementById(active) : null
  return chosen?.dataset.commit ?? null
}

/**
 * What can be done to a commit of the graph, on a right click or the menu key:
 * check out a branch that is on it, check the commit itself out on no branch,
 * or make a branch or a tag on it.
 */
export function CommitMenu({ path, sidebar, children }: { path: string; sidebar: Sidebar | null; children: ReactNode }) {
  const { t } = useTranslation()
  const [commit, setCommit] = useState<string | null>(null)
  const [open, setOpen] = useState(false)
  const [asking, setAsking] = useState<Asking | null>(null)
  const busy = useReach(path).run !== null
  const onIt = sidebar?.refs.branches.filter((branch) => branch.target === commit && !branch.head) ?? []
  const here = sidebar?.head.detached === true && sidebar.head.commit === commit

  return (
    <>
      <ContextMenu
        open={open}
        onOpenChange={(next, details) => {
          if (!next) {
            setOpen(false)
            return
          }
          const at = commitOf(details.event?.target ?? null)
          if (at !== null && sidebar !== null) {
            setCommit(at)
            setOpen(true)
          }
        }}
      >
        <ContextMenuTrigger render={<div className="h-full min-h-0" />}>{children}</ContextMenuTrigger>
        <ContextMenuPopup size="lg">
          {commit !== null && (
            <>
              {onIt.map((branch) => (
                <ContextMenuItem key={branch.name} disabled={busy} onClick={() => void checkoutBranch(path, branch.name)}>
                  {t('refs.checkout', { name: branch.name })}
                </ContextMenuItem>
              ))}
              <ContextMenuItem disabled={busy || here} onClick={() => void checkoutCommit(path, commit)}>
                {t('refs.checkoutCommit', { at: short(commit) })}
              </ContextMenuItem>
              <ContextMenuSeparator />
              <ContextMenuItem
                onClick={() => setAsking({ kind: 'branch', from: commit, label: short(commit), at: short(commit) })}
              >
                {t('refs.newBranchHere')}
              </ContextMenuItem>
              <ContextMenuItem onClick={() => setAsking({ kind: 'tag', target: commit, at: short(commit) })}>
                {t('refs.newTagHere')}
              </ContextMenuItem>
            </>
          )}
        </ContextMenuPopup>
      </ContextMenu>
      {sidebar && <RefDialogs path={path} sidebar={sidebar} asking={asking} onDone={() => setAsking(null)} />}
    </>
  )
}
