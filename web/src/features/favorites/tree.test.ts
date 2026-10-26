import { describe, expect, it } from 'vitest'

import type { Favorite } from '../../shared/api'
import { buildTree, canMoveInto, defaultName, dropTarget, folderPath } from './tree'

function fav(id: string, parent: string | null, position: number, kind: Favorite['kind'] = 'item'): Favorite {
  return {
    id,
    parent_id: parent,
    kind,
    name: id,
    sql: kind === 'item' ? `SELECT '${id}'` : null,
    note: null,
    driver: null,
    variant: null,
    position,
    created_at: 0,
    updated_at: 0,
  }
}

// 根：a、F（文件夹，含 b、G（文件夹，含 c））、d
const FAVS = [
  fav('d', null, 2),
  fav('a', null, 0),
  fav('F', null, 1, 'folder'),
  fav('G', 'F', 1, 'folder'),
  fav('b', 'F', 0),
  fav('c', 'G', 0),
]

describe('buildTree', () => {
  it('按次序组成树', () => {
    const names = (nodes: ReturnType<typeof buildTree>): unknown[] =>
      nodes.map((n) => (n.children.length ? [n.fav.id, names(n.children)] : n.fav.id))
    expect(names(buildTree(FAVS))).toEqual(['a', ['F', ['b', ['G', ['c']]]], 'd'])
  })
})

describe('canMoveInto', () => {
  it('只能移到文件夹，且不能是自己或下级', () => {
    expect(canMoveInto(FAVS, 'a', 'F')).toBe(true)
    expect(canMoveInto(FAVS, 'a', null)).toBe(true)
    expect(canMoveInto(FAVS, 'a', 'b')).toBe(false)
    expect(canMoveInto(FAVS, 'F', 'F')).toBe(false)
    expect(canMoveInto(FAVS, 'F', 'G')).toBe(false)
    expect(canMoveInto(FAVS, 'G', null)).toBe(true)
  })
})

describe('dropTarget', () => {
  it('拖到前后：同级中的位置不算被拖动项', () => {
    expect(dropTarget(FAVS, 'a', 'd', 1)).toEqual({ parent_id: null, position: 2 })
    expect(dropTarget(FAVS, 'd', 'a', -1)).toEqual({ parent_id: null, position: 0 })
    expect(dropTarget(FAVS, 'a', 'b', -1)).toEqual({ parent_id: 'F', position: 0 })
    expect(dropTarget(FAVS, 'c', 'G', 1)).toEqual({ parent_id: 'F', position: 2 })
  })

  it('拖到文件夹里面放在最前；不能拖进自己的下级', () => {
    expect(dropTarget(FAVS, 'a', 'G', 0)).toEqual({ parent_id: 'G', position: 0 })
    expect(dropTarget(FAVS, 'a', 'b', 0)).toBeNull()
    expect(dropTarget(FAVS, 'F', 'G', 0)).toBeNull()
    expect(dropTarget(FAVS, 'F', 'c', 1)).toBeNull()
    expect(dropTarget(FAVS, 'a', 'a', 1)).toBeNull()
  })
})

describe('defaultName / folderPath', () => {
  it('合并换行和空白，过长时截断', () => {
    expect(defaultName('\n  SELECT  *\n FROM t')).toBe('SELECT * FROM t')
    const long = `SELECT ${'x'.repeat(80)}`
    expect(defaultName(long)).toHaveLength(60)
    expect(defaultName(long).endsWith('…')).toBe(true)
  })

  it('文件夹路径', () => {
    expect(folderPath(FAVS, 'G')).toEqual(['F', 'G'])
  })
})
