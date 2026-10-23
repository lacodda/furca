import { useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Archive, Box, Boxes, Folder, GitBranch, Globe, Tag as TagIcon } from 'lucide-react'
import { Alert } from '@/components/ui/alert'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Spinner } from '@/components/ui/spinner'
import { TreeView } from '@/components/ui/tree-view'
import type { TreeNode } from '@/components/ui/tree-rows'
import type { Branch } from '@/lib/generated/Branch'
import type { RemoteBranch } from '@/lib/generated/RemoteBranch'
import type { Sidebar as SidebarData } from '@/lib/generated/Sidebar'
import { commitOfColumnRow } from '@/lib/graph-labels'
import { aheadBehind, byVersionDescending, nameTree, type NameNode } from '@/lib/names'
import { useSidebar } from '@/lib/repos'
import { cn } from 'dowel-ui'

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
    </ScrollArea>
  )
}

type Translate = ReturnType<typeof useTranslation>['t']

function sections(data: SidebarData, t: Translate): TreeNode[] {
  const byRemote = new Map<string, RemoteBranch[]>()
  for (const branch of data.refs.remote_branches) {
    const remote = branch.remote ?? branch.name.split('/')[0] ?? branch.name
    byRemote.set(remote, [...(byRemote.get(remote) ?? []), branch])
  }
  const remoteNames = [...new Set([...data.remotes.map((remote) => remote.name), ...byRemote.keys()])]
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
