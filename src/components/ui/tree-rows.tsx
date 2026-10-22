import type { ReactNode } from 'react'

/*
 * What a tree shows, with no React in it.
 *
 * Split out of the TreeView for the reason `table-sort` and `calendar-math`
 * were split out of their components: these are the sums, and the component is
 * the thing that draws them. A product that windows a large tree needs to know
 * how many rows it has before rendering any of them - that is this file, and
 * it never touches the DOM.
 */

export interface TreeNode {
  /** Stable identity. Not the label: two folders can hold a file of the same
   * name, and keying by label collapses them into one. */
  id: string
  label: ReactNode
  /** Absent or empty means a leaf. An empty array is deliberately *not* an
   * empty folder - a folder with nothing in it still opens, and saying so
   * needs a real value rather than the absence of one. */
  children?: TreeNode[]
  /** An empty folder: opens, and shows nothing. */
  empty?: boolean
}

/** One visible row: the node, how deep it sits, and the branch it belongs to. */
export interface TreeRow {
  node: TreeNode
  depth: number
  /** The id of the folder this row is inside, if any. What Left uses to get
   * out of a deep branch in one press. */
  parent?: string
}

/** The rows a tree shows, in the order the eye and the keyboard travel.
 *
 * Flattening is what makes the keyboard simple: Down is the next row of this
 * list, Up the previous, and neither has to know about nesting. A recursive
 * walk at every keystroke would ask the same question - what is visually next
 * - and answer it differently at each depth.
 *
 * It is also the answer to "how many rows is this tree", which is what a
 * product needs to put a tree inside a VirtualList. */
export function visibleRows(
  nodes: readonly TreeNode[],
  open: ReadonlySet<string>,
  depth = 0,
  parent?: string,
): TreeRow[] {
  const rows: TreeRow[] = []
  for (const node of nodes) {
    rows.push({ node, depth, parent })
    if (node.children?.length && open.has(node.id)) {
      rows.push(...visibleRows(node.children, open, depth + 1, node.id))
    }
  }
  return rows
}

/** Whether this node is a folder - something that opens, even if empty. */
export function isBranch(node: TreeNode): boolean {
  return Boolean(node.empty) || Boolean(node.children?.length)
}
