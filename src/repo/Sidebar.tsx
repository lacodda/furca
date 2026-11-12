import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Archive, Box, Boxes, Folder, GitBranch, Globe, Tag as TagIcon } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import {
  ContextMenu,
  ContextMenuItem,
  ContextMenuPopup,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from '@/components/ui/context-menu'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Spinner } from '@/components/ui/spinner'
import { TreeView } from '@/components/ui/tree-view'
import type { TreeNode } from '@/components/ui/tree-rows'
import type { Branch } from '@/lib/generated/Branch'
import type { RemoteBranch } from '@/lib/generated/RemoteBranch'
import type { Sidebar as SidebarData } from '@/lib/generated/Sidebar'
import { columnRow, commitOfColumnRow, type ColumnRow } from '@/lib/graph-labels'
import { aheadBehind, byVersionDescending, nameTree, short, type NameNode } from '@/lib/names'
import { checkoutBranch, checkoutCommit, fetchFrom, pruneRemote, pushTag, unfollow, useReach } from '@/lib/remote'
import { useSidebar } from '@/lib/repos'
import { cn } from 'dowel-ui'
import { RefDialogs, type Asking } from './Refs'

/** The sections open when a repository is first shown: what a person looks
 * at most. Tags and stashes can be hundreds of rows, and open on asking. */
const FIRST_OPEN = ['branches', 'remotes']

/** Which folders are open, per repository, kept while the window is: going
 * back to a tab finds its column as it was left. */
const openByRepository = new Map<string, Set<string>>()

/**
 * The left column: Branches, Remotes, Tags, Stashes and Submodules, as Fork
 * lays them out - one tree with one tab stop, arrows to move. Choosing a
 * branch, a tag or a stash entry takes the graph to its commit (`onJump`).
 * The counts beside a branch are how far it is from its upstream, as of the
 * last fetch.
 */
export function Sidebar({ path, onJump }: { path: string; onJump?: (commit: string) => void }) {
  const { t } = useTranslation()
  const read = useSidebar(path)
  const [open, setOpen] = useState<Set<string>>(
    () => openByRepository.get(path) ?? new Set(FIRST_OPEN),
  )
  const [chosen, setChosen] = useState<string | undefined>(undefined)

  if (read.state === 'reading') {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner label={t('column.reading')} />
      </div>
    )
  }
  if (read.state === 'failed') {
    return (
      <div className="p-3">
        <Alert tone="bad" title={t('column.failed')}>
          {read.reason}
        </Alert>
      </div>
    )
  }

  return (
    <ScrollArea label={t('column.label')} orientation="vertical" className="h-full">
      <RowMenu path={path} data={read.value}>
        <TreeView
          label={t('column.label')}
          nodes={sections(read.value, t)}
          open={open}
          onOpenChange={(next) => {
            openByRepository.set(path, next)
            setOpen(next)
          }}
          selected={chosen}
          onSelect={(id) => {
            setChosen(id)
            const commit = commitOfColumnRow(read.value, id)
            if (commit !== null) onJump?.(commit)
          }}
          className="p-1.5"
        />
      </RowMenu>
    </ScrollArea>
  )
}

/** The row of the column a context menu was asked for over: the one under the
 * pointer, or - from the keyboard, where the event comes from the tree itself
 * - the one the tree's cursor is on. */
function rowOf(target: EventTarget | null): string | null {
  if (!(target instanceof Element)) return null
  const row = target.closest('[role=treeitem]') ?? (target.matches('[role=tree]') ? target : target.closest('[role=tree]'))
  const id = row?.getAttribute('role') === 'tree' ? row.getAttribute('aria-activedescendant') : row?.id
  return id?.startsWith('tree-') ? id.slice('tree-'.length) : null
}

/** Every remote the column shows: the configured ones, and any a
 * remote-tracking branch names without one. */
function remoteNamesOf(data: SidebarData): string[] {
  const named = data.refs.remote_branches.map((branch) => branch.remote ?? branch.name.split('/')[0] ?? branch.name)
  return [...new Set([...data.remotes.map((remote) => remote.name), ...named])]
}

/** Whether a row has anything to do to it, and so a menu. */
function hasMenu(row: ColumnRow, data: SidebarData): boolean {
  switch (row.kind) {
    case 'section':
      return row.section === 'branches' || row.section === 'remotes' || row.section === 'tags'
    case 'remote':
      return data.remotes.some((remote) => remote.name === row.remote)
    case 'branch':
    case 'remote-branch':
    case 'tag':
      return true
    case 'other':
      return false
  }
}

/**
 * What can be done to a row of the column, on a right click or the menu key:
 * to a branch, check it out, branch from it, rename it, set what it follows,
 * delete it; to a remote branch, check it out as a local one; to a tag, check
 * it out on no branch, push it, delete it; to a remote, fetch it, prune it,
 * change it or remove it; to a section, make one of what it holds. Rows with
 * nothing to do open no menu.
 */
function RowMenu({ path, data, children }: { path: string; data: SidebarData; children: ReactNode }) {
  const [row, setRow] = useState<ColumnRow | null>(null)
  const [open, setOpen] = useState(false)
  const [asking, setAsking] = useState<Asking | null>(null)

  return (
    <>
      <ContextMenu
        open={open}
        onOpenChange={(next, details) => {
          if (!next) {
            setOpen(false)
            return
          }
          const id = rowOf(details.event?.target ?? null)
          const at = id === null ? null : columnRow(id, remoteNamesOf(data))
          if (at !== null && hasMenu(at, data)) {
            setRow(at)
            setOpen(true)
          }
        }}
      >
        <ContextMenuTrigger render={<div />}>{children}</ContextMenuTrigger>
        <ContextMenuPopup size="lg">{row && <RowItems path={path} data={data} row={row} onAsk={setAsking} />}</ContextMenuPopup>
      </ContextMenu>
      <RefDialogs path={path} sidebar={data} asking={asking} onDone={() => setAsking(null)} />
    </>
  )
}

function RowItems({
  path,
  data,
  row,
  onAsk,
}: {
  path: string
  data: SidebarData
  row: ColumnRow
  onAsk: (asking: Asking) => void
}) {
  const { t } = useTranslation()
  const busy = useReach(path).run !== null
  const remotes = data.remotes
  const head = data.head.commit

  switch (row.kind) {
    case 'section':
      if (row.section === 'branches') {
        return (
          <ContextMenuItem
            disabled={head === null}
            onClick={() =>
              onAsk({ kind: 'branch', from: 'HEAD', label: data.head.branch ?? short(head ?? ''), at: short(head ?? '') })
            }
          >
            {t('refs.newBranch')}
          </ContextMenuItem>
        )
      }
      if (row.section === 'tags') {
        return (
          <ContextMenuItem
            disabled={head === null}
            onClick={() => onAsk({ kind: 'tag', target: head ?? 'HEAD', at: short(head ?? '') })}
          >
            {t('refs.newTag')}
          </ContextMenuItem>
        )
      }
      return (
        <>
          <ContextMenuItem disabled={busy || remotes.length === 0} onClick={() => void fetchFrom(path, { to: 'all' })}>
            {t('reach.fetchAll')}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem onClick={() => onAsk({ kind: 'add-remote' })}>{t('remote.add')}</ContextMenuItem>
        </>
      )
    case 'remote':
      return (
        <>
          <ContextMenuItem disabled={busy} onClick={() => void fetchFrom(path, { to: 'named', name: row.remote })}>
            {t('remote.fetch', { name: row.remote })}
          </ContextMenuItem>
          <ContextMenuItem disabled={busy} onClick={() => void pruneRemote(path, row.remote)}>
            {t('remote.prune', { name: row.remote })}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem disabled={busy} onClick={() => onAsk({ kind: 'edit-remote', remote: row.remote })}>
            {t('remote.edit')}
          </ContextMenuItem>
          <ContextMenuItem tone="danger" disabled={busy} onClick={() => onAsk({ kind: 'remove-remote', remote: row.remote })}>
            {t('remote.removeMenu')}
          </ContextMenuItem>
        </>
      )
    case 'branch': {
      const branch = data.refs.branches.find((each) => each.name === row.name)
      if (!branch) return null
      const at = short(branch.target)
      return (
        <>
          <ContextMenuItem disabled={busy || branch.head} onClick={() => void checkoutBranch(path, branch.name)}>
            {t('refs.checkout', { name: branch.name })}
          </ContextMenuItem>
          <ContextMenuItem onClick={() => onAsk({ kind: 'branch', from: branch.name, label: branch.name, at })}>
            {t('refs.newBranchFrom', { name: branch.name })}
          </ContextMenuItem>
          <ContextMenuItem onClick={() => onAsk({ kind: 'tag', target: branch.target, at })}>
            {t('refs.newTagOn', { name: branch.name })}
          </ContextMenuItem>
          <ContextMenuSeparator />
          <ContextMenuItem disabled={busy} onClick={() => onAsk({ kind: 'rename', branch: branch.name })}>
            {t('refs.renameMenu')}
          </ContextMenuItem>
          <ContextMenuItem
            disabled={busy || data.refs.remote_branches.length === 0}
            onClick={() => onAsk({ kind: 'follow', branch })}
          >
            {t('refs.followMenu')}
          </ContextMenuItem>
          {branch.upstream && (
            <ContextMenuItem disabled={busy} onClick={() => void unfollow(path, branch.name)}>
              {t('refs.unfollow', { upstream: branch.upstream.name })}
            </ContextMenuItem>
          )}
          <ContextMenuSeparator />
          <ContextMenuItem
            tone="danger"
            disabled={busy || branch.head}
            title={branch.head ? t('refs.deleteHead') : undefined}
            onClick={() => onAsk({ kind: 'delete-branch', branch })}
          >
            {t('refs.deleteMenu')}
          </ContextMenuItem>
        </>
      )
    }
    case 'remote-branch': {
      const branch = data.refs.remote_branches.find((each) => each.name === row.name)
      if (!branch) return null
      const local = row.name.slice(row.remote.length + 1)
      const existing = data.refs.branches.find((each) => each.name === local)
      return (
        <>
          <ContextMenuItem disabled={busy || existing?.head === true} onClick={() => void checkoutBranch(path, row.name)}>
            {existing ? t('refs.checkout', { name: local }) : t('refs.checkoutAs', { name: local })}
          </ContextMenuItem>
          <ContextMenuItem
            onClick={() => onAsk({ kind: 'branch', from: row.name, label: row.name, at: short(branch.target), follows: local })}
          >
            {t('refs.newBranchFrom', { name: row.name })}
          </ContextMenuItem>
        </>
      )
    }
    case 'tag': {
      const tag = data.refs.tags.find((each) => each.name === row.name)
      if (!tag) return null
      const only = remotes.length === 1 ? remotes[0] : undefined
      return (
        <>
          <ContextMenuItem disabled={busy} onClick={() => void checkoutCommit(path, `refs/tags/${tag.name}`)}>
            {t('refs.checkoutTag', { name: tag.name })}
          </ContextMenuItem>
          <ContextMenuItem onClick={() => onAsk({ kind: 'branch', from: tag.name, label: tag.name, at: short(tag.target) })}>
            {t('refs.newBranchFrom', { name: tag.name })}
          </ContextMenuItem>
          {remotes.length > 0 && <ContextMenuSeparator />}
          {only !== undefined ? (
            <ContextMenuItem disabled={busy} onClick={() => void pushTag(path, tag.name, { to: 'named', name: only.name })}>
              {t('refs.pushTag', { name: tag.name, remote: only.name })}
            </ContextMenuItem>
          ) : (
            remotes.length > 1 && (
              <>
                <ContextMenuItem disabled={busy} onClick={() => void pushTag(path, tag.name, { to: 'all' })}>
                  {t('refs.pushTagAll', { name: tag.name })}
                </ContextMenuItem>
                {remotes.map((remote) => (
                  <ContextMenuItem
                    key={remote.name}
                    disabled={busy}
                    onClick={() => void pushTag(path, tag.name, { to: 'named', name: remote.name })}
                  >
                    {t('refs.pushTag', { name: tag.name, remote: remote.name })}
                  </ContextMenuItem>
                ))}
              </>
            )
          )}
          <ContextMenuSeparator />
          <ContextMenuItem tone="danger" disabled={busy} onClick={() => onAsk({ kind: 'delete-tag', tag: tag.name })}>
            {t('refs.deleteMenu')}
          </ContextMenuItem>
        </>
      )
    }
    case 'other':
      return null
  }
}

type Translate = ReturnType<typeof useTranslation>['t']

function sections(data: SidebarData, t: Translate): TreeNode[] {
  const byRemote = new Map<string, RemoteBranch[]>()
  for (const branch of data.refs.remote_branches) {
    const remote = branch.remote ?? branch.name.split('/')[0] ?? branch.name
    byRemote.set(remote, [...(byRemote.get(remote) ?? []), branch])
  }
  const remoteNames = remoteNamesOf(data)
  const tags = [...data.refs.tags].sort((a, b) => byVersionDescending(a.name, b.name))

  return [
    section(
      'branches',
      t('column.branches'),
      <GitBranch aria-hidden />,
      data.refs.branches.length,
      folders('b', nameTree(data.refs.branches, (b) => b.name), branchLabel),
    ),
    section(
      'remotes',
      t('column.remotes'),
      <Globe aria-hidden />,
      remoteNames.length,
      remoteNames.map((name) => {
        const remote = data.remotes.find((candidate) => candidate.name === name)
        const branches = byRemote.get(name) ?? []
        const tree = nameTree(branches, (branch) => branch.name.slice(name.length + 1))
        return {
          id: `r:${name}`,
          label: (
            <Labelled icon={<Globe aria-hidden />} title={remote?.url ?? undefined}>
              {name}
            </Labelled>
          ),
          children: folders(`r:${name}`, tree, (branch) => (
            <Labelled icon={<GitBranch aria-hidden />}>{branch.name.split('/').at(-1)}</Labelled>
          )),
          empty: branches.length === 0,
        }
      }),
    ),
    section(
      'tags',
      t('column.tags'),
      <TagIcon aria-hidden />,
      tags.length,
      tags.map((tag) => ({
        id: `t:${tag.name}`,
        label: <Labelled icon={<TagIcon aria-hidden />}>{tag.name}</Labelled>,
      })),
    ),
    section(
      'stashes',
      t('column.stashes'),
      <Archive aria-hidden />,
      data.stashes.length,
      data.stashes.map((stash) => ({
        id: `s:${stash.index}`,
        label: (
          <Labelled icon={<Archive aria-hidden />} title={`stash@{${String(stash.index)}}: ${stash.message}`}>
            {stash.message}
          </Labelled>
        ),
      })),
    ),
    section(
      'submodules',
      t('column.submodules'),
      <Boxes aria-hidden />,
      data.submodules.length,
      data.submodules.map((module) => ({
        id: `m:${module.path}`,
        label: (
          <Labelled icon={<Box aria-hidden />} title={module.url ?? undefined}>
            {module.path}
            {module.checked_out === null ? (
              <span className="ml-1.5 text-2xs text-faint">{t('column.notCheckedOut')}</span>
            ) : module.checked_out !== module.recorded ? (
              <span className="ml-1.5 text-2xs text-warn">{t('column.moved')}</span>
            ) : null}
          </Labelled>
        ),
      })),
    ),
  ]
}

/** A top-level section: its name and how many it holds. */
function section(id: string, name: string, icon: ReactNode, total: number, children: TreeNode[]): TreeNode {
  return {
    id,
    label: (
      <Labelled icon={icon} className="font-medium">
        {name}
        <span className="ml-1.5 font-mono text-2xs font-normal text-faint">{total}</span>
      </Labelled>
    ),
    children,
    empty: children.length === 0,
  }
}

/** A tree of names as tree nodes: folders for the parts before a `/`, the
 * item drawn by `leaf` where there is one. */
function folders<T>(prefix: string, nodes: NameNode<T>[], leaf: (item: T) => ReactNode): TreeNode[] {
  return nodes.map((node) => {
    if (node.children.length === 0 && node.item !== undefined) {
      return { id: `${prefix}:${node.id}`, label: leaf(node.item) }
    }
    const children = folders(prefix, node.children, leaf)
    return {
      id: `${prefix}/${node.id}`,
      label: <Labelled icon={<Folder aria-hidden />}>{node.label}</Labelled>,
      children: node.item === undefined ? children : [{ id: `${prefix}:${node.id}`, label: leaf(node.item) }, ...children],
    }
  })
}

function branchLabel(branch: Branch): ReactNode {
  const last = branch.name.split('/').at(-1) ?? branch.name
  const sync = branch.upstream ? aheadBehind(branch.upstream) : undefined
  return (
    <Labelled
      icon={<GitBranch aria-hidden />}
      iconClassName={branch.head ? 'text-accent' : undefined}
      className={cn(branch.head && 'font-semibold')}
      title={branch.upstream ? `${branch.name} → ${branch.upstream.name}` : branch.name}
    >
      {last}
      {sync === null ? (
        <Gone />
      ) : sync && (sync.ahead > 0 || sync.behind > 0) ? (
        <span className="ml-1.5 font-mono text-2xs font-normal text-dim">
          {sync.ahead > 0 && `↑${String(sync.ahead)}`}
          {sync.ahead > 0 && sync.behind > 0 && ' '}
          {sync.behind > 0 && `↓${String(sync.behind)}`}
        </span>
      ) : null}
    </Labelled>
  )
}

function Gone() {
  const { t } = useTranslation()
  return <span className="ml-1.5 text-2xs font-normal text-faint">{t('column.gone')}</span>
}

/** A row's words after its icon. The icon is dim unless `iconClassName`
 * says otherwise - the branch HEAD is on takes the accent. */
function Labelled({
  icon,
  children,
  className,
  iconClassName,
  title,
}: {
  icon: ReactNode
  children: ReactNode
  className?: string
  iconClassName?: string
  title?: string
}) {
  return (
    <span className={cn('flex min-w-0 items-center gap-1.5', className)} title={title}>
      <span className={cn('flex shrink-0 [&>svg]:size-3.5', iconClassName ?? 'text-dim')}>{icon}</span>
      <span className="truncate">{children}</span>
    </span>
  )
}
