import { describe, expect, it } from 'vitest'

import type { DesignForeignKey, DesignTableModel, ErTable, LogicalKey } from '../../shared/api'
import { HEADER_HEIGHT, ROW_HEIGHT, TABLE_HANDLE, buildGraph, layout, nodeHeight, searchTables } from './graph'
import { dataUrlBase64, imageName } from './export'

const fk = (columns: string[], ref_table: string, ref_columns: string[], extra: Partial<DesignForeignKey> = {}): DesignForeignKey => ({
  name: `fk_${columns.join('_')}`,
  columns,
  ref_schema: null,
  ref_table,
  ref_columns,
  on_delete: null,
  on_update: null,
  ...extra,
})

function table(name: string, cols: string[], fks: DesignForeignKey[] = []): DesignTableModel {
  return {
    name,
    comment: null,
    columns: cols.map((c, i) => ({
      name: c,
      ordinal: i + 1,
      data_type: { native: 'integer', logical: null },
      nullable: i > 0,
      default: null,
      comment: null,
      auto_increment: false,
      generated: null,
    })),
    primary_key: { name: null, columns: [cols[0]] },
    uniques: [],
    foreign_keys: fks,
    checks: [],
    indexes: [],
    triggers: [],
    partition: null,
  }
}

const er = (t: DesignTableModel, logical_keys: LogicalKey[] = []): ErTable => ({ table: t, logical_keys })

/** customers；orders 的 (cust_id, cust_code) 复合外键指向 customers，parent_id 自引用，region_id 指向 crm.regions；
 * items 的逻辑外键指向 orders。 */
function sample(): ErTable[] {
  return [
    er(table('customers', ['id', 'code', 'name'])),
    er(
      table('orders', ['id', 'cust_id', 'cust_code', 'parent_id', 'region_id', 'note'], [
        fk(['cust_id', 'cust_code'], 'customers', ['id', 'code']),
        fk(['parent_id'], 'orders', ['id']),
        fk(['region_id'], 'regions', ['id'], { ref_schema: 'crm' }),
      ]),
    ),
    er(table('items', ['id', 'order_ref']), [{ name: 'lk_order', columns: ['order_ref'], ref_table: 'orders', ref_columns: ['id'] }]),
  ]
}

describe('ER 图的节点和连线', () => {
  it('复合外键一条线、自引用、跨 schema 列在节点上、逻辑外键为虚线', () => {
    const g = buildGraph(sample(), 'columns', 'public')
    expect(g.nodes.map((n) => n.id)).toEqual(['customers', 'orders', 'items'])
    expect(g.edges.map((e) => [e.id, e.source, e.target, e.sourceHandle, e.targetHandle, e.count, e.logical])).toEqual([
      ['fk:orders:0', 'orders', 'customers', 'cust_id:out', 'id:in', 2, false],
      ['fk:orders:1', 'orders', 'orders', 'parent_id:out', 'id:in', 1, false],
      ['lk:items:0', 'items', 'orders', 'order_ref:out', 'id:in', 1, true],
    ])
    expect(g.edges[0].title).toBe('fk_cust_id_cust_code: cust_id → id, cust_code → code')
    expect(g.edges[2].logicalIndex).toBe(0)
    const orders = g.nodes[1]
    expect(orders.data.external).toEqual(['→ crm.regions'])
    expect(orders.data.columns.find((c) => c.name === 'cust_id')).toMatchObject({ fk: true, pk: false, nullable: true })
    expect(orders.height).toBe(nodeHeight(6, 1))
  })

  it('主题图中没有的表列在节点上；只显示键列、只显示表名', () => {
    const [customers, orders] = sample()
    const g = buildGraph([orders], 'columns', 'public')
    expect(g.nodes[0].data.external).toEqual(['→ customers', '→ crm.regions'])
    expect(g.edges.map((e) => e.id)).toEqual(['fk:orders:1'])
    const keys = buildGraph([customers, orders], 'keys', 'public')
    expect(keys.nodes[0].data.columns.map((c) => c.name)).toEqual(['id', 'code'])
    expect(keys.nodes[1].data.columns.map((c) => c.name)).toEqual(['id', 'cust_id', 'cust_code', 'parent_id', 'region_id'])
    const names = buildGraph([customers, orders], 'names', 'public')
    expect(names.nodes[0].data.columns).toEqual([])
    expect(names.nodes[0].height).toBe(HEADER_HEIGHT + 6)
    expect(names.edges[0]).toMatchObject({ sourceHandle: `${TABLE_HANDLE}:out`, targetHandle: `${TABLE_HANDLE}:in` })
    expect(ROW_HEIGHT).toBeGreaterThan(0)
  })

  it('文件有错的表只有名字', () => {
    const bad: ErTable = { table: table('bad', ['id']), logical_keys: [], error: { path: 'tables/public/bad.yaml', message: 'line 1' } }
    const g = buildGraph([bad], 'columns', 'public')
    expect(g.nodes[0].data).toMatchObject({ error: 'line 1', columns: [] })
  })
})

describe('自动布局', () => {
  it('全都没有位置时用 dagre；有位置时保留，只把新表排在下方', () => {
    const g = buildGraph(sample(), 'columns', 'public')
    const auto = layout(g, {})
    expect(Object.keys(auto).sort()).toEqual(['customers', 'items', 'orders'])
    // 横向：被引用的 customers 在 orders 右边或左边，但 items → orders → customers 依次排开
    expect(new Set([auto.items.x, auto.orders.x, auto.customers.x]).size).toBe(3)
    const kept = layout(g, { customers: { x: 0, y: 0 }, orders: { x: 400, y: 0 } })
    expect(kept.customers).toEqual({ x: 0, y: 0 })
    expect(kept.orders).toEqual({ x: 400, y: 0 })
    expect(kept.items.y).toBe(nodeHeight(6, 1) + 80)
    const all = layout(g, { customers: { x: 0, y: 0 } }, { all: true, direction: 'TB' })
    expect(all.customers).not.toEqual({ x: 0, y: 0 })
  })

  it('搜索表名', () => {
    expect(searchTables(sample().map((x) => x.table), 'ORD')).toEqual(['orders'])
    expect(searchTables(sample().map((x) => x.table), ' ')).toEqual([])
  })
})

describe('导出', () => {
  it('data URL 转 base64、文件名', () => {
    expect(dataUrlBase64('data:image/png;base64,iVBOR')).toBe('iVBOR')
    expect(atob(dataUrlBase64('data:image/svg+xml;charset=utf-8,%3Csvg%3E'))).toBe('<svg>')
    expect(imageName('订单/明细', 'svg')).toBe('订单_明细.svg')
  })
})
