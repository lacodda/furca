import type { Tab } from './generated/Tab'
import type { Tracking } from './generated/Tracking'

/*
 * The sums behind what the window prints, with no React in them, so each is
 * tested on its own.
 */

/** A node of a tree of names split at `/`: `feature/login` is a folder
 * `feature` holding a leaf `login`. */
export interface NameNode<T> {
  /** The full name up to this node, which is what makes it unique. */
  id: string
  /** The last part of the name, which is what the row says. */
  label: string
  /** The item at exactly this name, if there is one. */
  item?: T
  children: NameNode<T>[]
}

/** Arranges `items` into folders by the `/` in their names, the way Fork
 * and git's own tools draw branches: `feature/a` and `feature/b` sit in one
 * `feature` folder. A name that is both a leaf and a folder - `release` and
 * `release/1.0` - keeps both: the folder carries the item. Folders come
 * before leaves; each group keeps the order the items came in. */
export function nameTree<T>(items: readonly T[], name: (item: T) => string): NameNode<T>[] {
  const root: NameNode<T> = { id: '', label: '', children: [] }
  for (const item of items) {
    const parts = name(item).split('/').filter((part) => part !== '')
    let node = root
    parts.forEach((part, index) => {
      const id = node.id === '' ? part : `${node.id}/${part}`
      let child = node.children.find((candidate) => candidate.label === part)
      if (!child) {
        child = { id, label: part, children: [] }
        node.children.push(child)
      }
      if (index === parts.length - 1) child.item = item
      node = child
    })
  }
  const order = (nodes: NameNode<T>[]): NameNode<T>[] => {
    const folders = nodes.filter((node) => node.children.length > 0)
    const leaves = nodes.filter((node) => node.children.length === 0)
    for (const folder of folders) folder.children = order(folder.children)
    return [...folders, ...leaves]
  }
  return order(root.children)
}

/** Tags newest version first: `v0.10.0` before `v0.9.0`, which a sort by
 * text gets the wrong way round. Numbers inside a name compare as numbers. */
export function byVersionDescending(a: string, b: string): number {
  return b.localeCompare(a, 'en', { numeric: true, sensitivity: 'base' })
}

/** What each tab says. A name is the repository's directory; two open
 * repositories with the same directory name each add their parent's name,
 * so the bar never shows two tabs it cannot tell apart. */
export function tabLabels(tabs: readonly Tab[]): string[] {
  const counts = new Map<string, number>()
  for (const tab of tabs) counts.set(tab.name, (counts.get(tab.name) ?? 0) + 1)
  return tabs.map((tab) => {
    if ((counts.get(tab.name) ?? 0) < 2) return tab.name
    const parts = tab.path.split(/[\\/]/).filter((part) => part !== '')
    const parent = parts[parts.length - 2]
    return parent === undefined ? tab.name : `${tab.name} · ${parent}`
  })
}

/** A branch against its upstream, as arrows and counts: `↑2 ↓1`, nothing when
 * the two are level, and `null` for an upstream that is gone. */
export function aheadBehind(tracking: Tracking): { ahead: number; behind: number } | null {
  if (tracking.ahead === null || tracking.behind === null) return null
  return { ahead: tracking.ahead, behind: tracking.behind }
}

/** The first seven characters of an object id, as git abbreviates it. */
export function short(id: string): string {
  return id.slice(0, 7)
}
