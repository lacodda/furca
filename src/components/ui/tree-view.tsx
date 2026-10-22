import { useMemo, useState, type KeyboardEvent } from 'react'
import { cn } from 'dowel-ui'
import { isBranch, visibleRows, type TreeNode } from './tree-rows'

/*
 * A tree: folders that open, one tab stop, arrows to move.
 *
 * The shape products reach for and then get wrong in the same place every
 * time. A tree is not a nest of lists with click handlers - it is one control
 * with a cursor in it, and the difference is the whole component:
 *
 *   **One tab stop, not one per node.** A tree of four hundred files with a
 *   `tabIndex` on each is four hundred stops between the sidebar and the
 *   editor. The container is what the keyboard reaches, and the arrows move a
 *   cursor inside it - the arrangement a `RadioGroup` has, for the same
 *   reason.
 *
 *   **Right and left do different things depending on where you are.** Right
 *   on a closed folder opens it; on an open one it steps into the first child;
 *   on a leaf it does nothing. Left closes an open folder, and on a leaf or a
 *   closed one it jumps to the parent - which is how a reader gets out of a
 *   deep branch without walking back up through every sibling.
 *
 * Both come from the ARIA tree pattern, and neither is guessable from looking
 * at a finished tree, which is why every hand-rolled one implements Up and
 * Down and stops there.
 *
 * The nodes are given, not discovered: a tree that loads its children when a
 * folder opens is a different component with a different problem (a spinner
 * per node, a request per open, an error state inside a row). This one is
 * handed everything and decides what to show.
 *
 * Which rows are visible is next door in `tree-rows`, with no React in it - a
 * product windowing a large tree imports that and never this.
 */

export interface TreeViewProps {
  nodes: readonly TreeNode[]
  /** Which folders are open. Uncontrolled if omitted. */
  open?: ReadonlySet<string>
  onOpenChange?: (open: Set<string>) => void
  /** Which row is chosen - the file being edited, not the one the cursor is
   * on. The two move apart: a reader walks the tree with arrows without
   * opening anything until Enter. */
  selected?: string
  onSelect?: (id: string) => void
  /** What a screen reader calls the tree. Required: a bare tree announces
   * "tree" and nothing else, and the word belongs to the product. */
  label: string
  className?: string
}

export function TreeView({
  nodes,
  open: openProp,
  onOpenChange,
  selected,
  onSelect,
  label,
  className,
}: TreeViewProps) {
  const [openState, setOpenState] = useState<ReadonlySet<string>>(() => new Set())
  const open = openProp ?? openState

  const setOpen = (next: Set<string>) => {
    if (openProp === undefined) setOpenState(next)
    onOpenChange?.(next)
  }

  const rows = useMemo(() => visibleRows(nodes, open), [nodes, open])

  /* Where the keyboard is, which is not where the selection is. The cursor
   * starts on the selected row when there is one, so arrowing into a tree
   * continues from what is open rather than from the top. */
  const [cursor, setCursor] = useState<string | undefined>(undefined)
  const active = rows.find((row) => row.node.id === cursor)
    ? cursor
    : (rows.find((row) => row.node.id === selected)?.node.id ?? rows[0]?.node.id)

  const index = rows.findIndex((row) => row.node.id === active)

  const move = (to: number) => {
    const row = rows[Math.min(Math.max(0, to), rows.length - 1)]
    if (row) setCursor(row.node.id)
  }

  const toggle = (id: string, next: boolean) => {
    const copy = new Set(open)
    if (next) copy.add(id)
    else copy.delete(id)
    setOpen(copy)
  }

  const onKeyDown = (event: KeyboardEvent) => {
    const current = rows[index]
    if (!current) return
    const branch = isBranch(current.node)
    const isOpen = open.has(current.node.id)

    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault()
        move(index + 1)
        break
      case 'ArrowUp':
        event.preventDefault()
        move(index - 1)
        break
      case 'ArrowRight':
        event.preventDefault()
        // Open, then step in. Two presses rather than one that does both,
        // because opening a folder and moving into it are different intents -
        // and a reader who wanted only to look inside would have lost the row
        // they were on.
        if (branch && !isOpen) toggle(current.node.id, true)
        else if (branch && isOpen) move(index + 1)
        break
      case 'ArrowLeft': {
        event.preventDefault()
        if (branch && isOpen) {
          toggle(current.node.id, false)
          break
        }
        // Out of the branch rather than up one row. This is the escape from a
        // deep folder, and the part hand-rolled trees leave out: without it
        // the only way back to the parent is through every sibling below.
        const parent = current.parent
        if (parent) setCursor(parent)
        break
      }
      case 'Home':
        event.preventDefault()
        move(0)
        break
      case 'End':
        event.preventDefault()
        move(rows.length - 1)
        break
      case 'Enter':
      case ' ':
        event.preventDefault()
        if (branch) toggle(current.node.id, !isOpen)
        else onSelect?.(current.node.id)
        break
      default:
        break
    }
  }

  return (
    <div
      role="tree"
      aria-label={label}
      // One stop for the whole tree. A `tabIndex` per node is how a sidebar of
      // four hundred files becomes four hundred stops before the editor.
      tabIndex={0}
      onKeyDown={onKeyDown}
      // The cursor is published rather than only drawn: this is what a screen
      // reader follows, and what makes the tree one control instead of a
      // collection of rows.
      aria-activedescendant={active ? `tree-${active}` : undefined}
      className={cn(
        'text-sm',
        'focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent',
        className,
      )}
    >
      {rows.map(({ node, depth }) => {
        const branch = isBranch(node)
        const isOpen = open.has(node.id)
        return (
          <div
            key={node.id}
            id={`tree-${node.id}`}
            role="treeitem"
            // Only on folders. On a leaf it would announce "collapsed" for
            // something that never opens.
            aria-expanded={branch ? isOpen : undefined}
            aria-selected={node.id === selected || undefined}
            // One-based, and the level a reader is told - the indent is the
            // sighted half of the same fact.
            aria-level={depth + 1}
            onClick={() => {
              setCursor(node.id)
              if (branch) toggle(node.id, !isOpen)
              else onSelect?.(node.id)
            }}
            className={cn(
              'flex cursor-pointer items-center gap-1 rounded-sm py-1 pr-2 transition-colors',
              node.id === selected ? 'bg-accent-soft text-text' : 'hover:bg-soft',
              node.id === active && node.id !== selected && 'bg-softer',
            )}
            // Indent by depth. A padding rather than a nested box, because the
            // rows are one flat list - which is what lets the keyboard treat
            // Down as "the next row" without knowing about nesting.
            style={{ paddingLeft: `${depth * 16 + 8}px` }}
          >
            <span aria-hidden className={cn('w-3 shrink-0 text-2xs text-dim', !branch && 'opacity-0')}>
              {isOpen ? '▾' : '▸'}
            </span>
            <span className="truncate">{node.label}</span>
          </div>
        )
      })}
    </div>
  )
}
