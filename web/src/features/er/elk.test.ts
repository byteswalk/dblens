import ELK from 'elkjs/lib/elk.bundled.js'
import { describe, expect, it } from 'vitest'

import type { DesignForeignKey, DesignTableModel, ErTable } from '../../shared/api'
import { fromElk, routePath, toElk } from './elk'
import { type Graph, buildGraph } from './graph'

const key = (ref: string, col: string) =>
  ({ name: null, columns: [col], ref_schema: null, ref_table: ref, ref_columns: ['id'], on_update: null, on_delete: null }) as DesignForeignKey

function er(name: string, cols: string[], foreign_keys: DesignForeignKey[] = []): ErTable {
  const table = {
    name,
    comment: null,
    columns: cols.map((c, i) => ({ name: c, ordinal: i + 1, data_type: { native: 'integer', logical: null }, nullable: true, default: null, comment: null })),
    primary_key: { name: null, columns: ['id'] },
    uniques: [],
    foreign_keys,
    checks: [],
    indexes: [],
    triggers: [],
    partition: null,
  } as unknown as DesignTableModel
  return { table, logical_keys: [] }
}

const sample = () =>
  buildGraph(
    [
      er('a', ['id']),
      er('b', ['id', 'a_id'], [key('a', 'a_id')]),
      er('c', ['id', 'a_id', 'b_id'], [key('a', 'a_id'), key('b', 'b_id')]),
      er('d', ['id', 'c_id', 'd_id'], [key('c', 'c_id'), key('d', 'd_id')]),
    ],
    'columns',
    'public',
  )

const overlaps = (g: Graph, p: Record<string, { x: number; y: number }>) => {
  let n = 0
  g.nodes.forEach((a, i) =>
    g.nodes.slice(i + 1).forEach((b) => {
      const [x, y] = [p[a.id], p[b.id]]
      if (x.x < y.x + b.width && y.x < x.x + a.width && x.y < y.y + b.height && y.y < x.y + a.height) n++
    }),
  )
  return n
}

describe('ELK 布局', () => {
  it('横向：表不重叠，连线有路线，自引用不进布局', async () => {
    const g = sample()
    const r = fromElk(await new ELK().layout(toElk(g, 'RIGHT')))
    expect(Object.keys(r.positions).sort()).toEqual(['a', 'b', 'c', 'd'])
    expect(overlaps(g, r.positions)).toBe(0)
    expect(Object.keys(r.routes).length).toBe(4)
    expect(Object.values(r.routes).every((p) => p.length >= 2)).toBe(true)
  })

  it('纵向也能排', async () => {
    const g = sample()
    const r = fromElk(await new ELK().layout(toElk(g, 'DOWN')))
    expect(overlaps(g, r.positions)).toBe(0)
  })

  it('按主题分组时，同主题的表聚在一起', async () => {
    const g = sample()
    const groups = new Map([['a', '甲'], ['b', '甲'], ['c', '乙'], ['d', '乙']])
    const r = fromElk(await new ELK().layout(toElk(g, 'RIGHT', groups)))
    expect(Object.keys(r.positions).sort()).toEqual(['a', 'b', 'c', 'd'])
    expect(overlaps(g, r.positions)).toBe(0)
    const box = (ids: string[]) => {
      const xs = ids.map((i) => r.positions[i].x)
      const ys = ids.map((i) => r.positions[i].y)
      return { l: Math.min(...xs), r: Math.max(...xs) + 240, t: Math.min(...ys), b: Math.max(...ys) + 80 }
    }
    const [m, n] = [box(['a', 'b']), box(['c', 'd'])]
    expect(m.r <= n.l || n.r <= m.l || m.b <= n.t || n.b <= m.t).toBe(true)
  })
})

describe('折线路径', () => {
  it('拐角带圆角，两点时是直线', () => {
    expect(routePath([{ x: 0, y: 0 }, { x: 10, y: 0 }])).toBe('M 0 0 L 10 0')
    expect(routePath([{ x: 0, y: 0 }, { x: 20, y: 0 }, { x: 20, y: 20 }])).toContain('Q 20 0')
  })
})
