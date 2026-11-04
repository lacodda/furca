import { useTranslation } from 'react-i18next'
import { File, FileBox, FileCode, Folder, Package } from 'lucide-react'
import { cn } from 'dowel-ui'
import { Badge } from '@/components/ui/badge'
import type { TreeNode } from '@/components/ui/tree-rows'
import { isRawBinary, sizeText, storedAs, type FileNode } from '@/lib/changes'
import type { FileChange } from '@/lib/generated/FileChange'

/*
 * The rows of a list of changed files: one for a commit's, and for each list
 * of the working copy. A row says the same things in both.
 */

/** A tree of files as the tree view's nodes, their ids under `prefix`: the same
 * path can be in two lists of the working copy at once. */
export function fileNodes(tree: readonly FileNode[], prefix = ''): TreeNode[] {
  return tree.map((node) => {
    const leaf = node.file === undefined ? null : { id: `${prefix}f:${node.file.path}`, label: <FileRow file={node.file} /> }
    if (node.children.length === 0) return leaf ?? { id: `${prefix}d:${node.id}`, label: node.label }
    const children = fileNodes(node.children, prefix)
    return {
      id: `${prefix}d:${node.id}`,
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
