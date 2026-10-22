import { describe, expect, it } from 'vitest'
import { aheadBehind, byVersionDescending, nameTree, short, tabLabels } from './names'

const labels = <T,>(nodes: ReturnType<typeof nameTree<T>>): unknown =>
  nodes.map((node) => (node.children.length ? { [node.label]: labels(node.children) } : node.label))

describe('nameTree', () => {
  it('puts names that share a prefix into one folder, folders first', () => {
    const tree = nameTree(['main', 'feature/login', 'feature/search', 'fix/a/b'], (name) => name)
    expect(labels(tree)).toEqual([{ feature: ['login', 'search'] }, { fix: [{ a: ['b'] }] }, 'main'])
  })

  it('gives every node the full name as its id, so two leaves of one label differ', () => {
    const tree = nameTree(['a/x', 'b/x'], (name) => name)
    const ids = tree.flatMap((folder) => folder.children.map((leaf) => leaf.id))
    expect(ids).toEqual(['a/x', 'b/x'])
  })

  it('keeps an item that is also a folder on the folder', () => {
    const tree = nameTree(['release', 'release/1.0'], (name) => name)
    expect(tree).toHaveLength(1)
    expect(tree[0]?.item).toBe('release')
    expect(tree[0]?.children[0]?.item).toBe('release/1.0')
  })
})

describe('byVersionDescending', () => {
  it('reads the numbers in a tag as numbers', () => {
    expect(['v0.9.0', 'v0.10.0', 'v0.2.1', 'v1.0.0'].sort(byVersionDescending)).toEqual([
      'v1.0.0',
      'v0.10.0',
      'v0.9.0',
      'v0.2.1',
    ])
  })
})

describe('tabLabels', () => {
  it('is the directory name, and adds the parent only where two would match', () => {
    const tabs = [
      { name: 'app', path: 'C:\\work\\app' },
      { name: 'app', path: '/home/ada/clones/app' },
      { name: 'site', path: '/srv/site' },
    ]
    expect(tabLabels(tabs)).toEqual(['app · work', 'app · clones', 'site'])
  })
})

describe('aheadBehind', () => {
  it('is null for an upstream that is gone', () => {
    expect(aheadBehind({ name: 'origin/x', ahead: null, behind: null })).toBeNull()
    expect(aheadBehind({ name: 'origin/x', ahead: 2, behind: 0 })).toEqual({ ahead: 2, behind: 0 })
  })
})

describe('short', () => {
  it('is the first seven characters, as git abbreviates', () => {
    expect(short('171154d8e3a0c1')).toBe('171154d')
  })
})
