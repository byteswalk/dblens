import { describe, expect, it } from 'vitest'

import type { CompareResult, DesignForeignKey, DesignTableModel, TableDiff } from '../../shared/api'
import { acceptSuggestion, compareLines, defaultSelection, dependencyIssues, EMPTY_RENAMES, tableText } from './logic'

const fk = (name: string, ref: string): DesignForeignKey => ({
  name,
  columns: ['ref_id'],
  ref_schema: null,
  ref_table: ref,
  ref_columns: ['id'],
  on_delete: null,
  on_update: null,
})

const table = (name: string, fks: DesignForeignKey[] = []): DesignTableModel => ({
  name,
  comment: null,
  columns: [
    { name: 'id', ordinal: 1, data_type: { native: 'integer', logical: null }, nullable: false, default: null, comment: null, auto_increment: false, generated: null },
  ],
  primary_key: { name: null, columns: ['id'] },
  uniques: [],
  foreign_keys: fks,
  checks: [],
  indexes: [],
  triggers: [],
  partition: null,
})

/** items 新建（1），orders 加指向 items 的外键（2）、删列（3）；old 删除（4），被 keep 的外键引用。 */
function result(): CompareResult {
  const tables: TableDiff[] = [
    { status: 'added', source: 'items', target: null, appended: true, items: [{ id: 1, change: { kind: 'table', change: { kind: 'create_table', table: table('items') } } }] },
    {
      status: 'changed',
      source: 'orders',
      target: 'orders',
      appended: true,
      items: [
        { id: 2, change: { kind: 'table', change: { kind: 'add_foreign_key', key: fk('fk_items', 'items') } } },
        { id: 3, change: { kind: 'table', change: { kind: 'drop_column', name: 'x' } } },
      ],
    },
    { status: 'removed', source: null, target: 'old', appended: true, items: [{ id: 4, change: { kind: 'drop_table', name: 'old' } }] },
    { status: 'same', source: 'keep', target: 'keep', appended: true, items: [] },
  ]
  return {
    diff: { tables, suggestions: [] },
    renames: EMPTY_RENAMES,
    source_errors: [],
    target_errors: [],
    source_tables: [],
    target_tables: [table('orders'), table('old'), table('keep', [fk('fk_old', 'old')])],
    fingerprint: 'f',
  }
}

describe('比对结果的勾选', () => {
  it('默认不勾选删除表、删除列', () => {
    expect(defaultSelection(result().diff.tables)).toEqual([1, 2])
  })

  it('依赖提示：外键引用的新表没有勾选建表、删除的表还被引用', () => {
    const r = result()
    expect(dependencyIssues(r, new Set([1, 2]))).toEqual([])
    expect(dependencyIssues(r, new Set([2]))).toEqual(['orders 的外键引用 items，但没有勾选新建 items'])
    expect(dependencyIssues(r, new Set([1, 2, 4]))).toEqual(['删除 old 时 keep 的外键仍引用它'])
  })
})

describe('两边定义对照', () => {
  it('表定义的文本与不同的行', () => {
    const a = table('t', [fk('fk_x', 'x')])
    const b = { ...table('t'), columns: [{ ...a.columns[0], data_type: { native: 'bigint', logical: null } }] }
    expect(tableText(a)).toEqual(['id integer NOT NULL', 'PRIMARY KEY (id)', 'FOREIGN KEY fk_x (ref_id) → x(id)'])
    const [l, r] = compareLines(tableText(a), tableText(b))
    expect(l.map((x) => x.diff)).toEqual([true, false, true])
    expect(r.map((x) => x.diff)).toEqual([true, false])
  })

  it('采用推测的改名', () => {
    const r = acceptSuggestion(EMPTY_RENAMES, { table: null, source: 'clients', target: 'customers' })
    const r2 = acceptSuggestion(r, { table: 'orders', source: 'total', target: 'amount' })
    expect(r2).toEqual({ tables: { clients: 'customers' }, columns: { orders: { total: 'amount' } } })
  })
})
