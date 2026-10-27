import { describe, expect, it } from 'vitest'

import type { DesignColumn, DesignTableModel, TypeEntry } from '../../shared/api'
import {
  addColumn,
  changedFields,
  columnState,
  emptyColumn,
  emptyDraft,
  fromTable,
  isDirty,
  joinType,
  moveColumn,
  removeColumn,
  renamesOf,
  restoreColumn,
  splitType,
  toModel,
  togglePrimaryKey,
  typeParams,
  updateColumn,
} from './logic'

const col = (name: string, native: string, extra: Partial<DesignColumn> = {}): DesignColumn => ({
  name,
  ordinal: 0,
  data_type: { native, logical: { kind: 'other' } },
  nullable: true,
  default: null,
  comment: null,
  auto_increment: false,
  generated: null,
  ...extra,
})

const table = (): DesignTableModel => ({
  name: 'orders',
  comment: '订单',
  columns: [col('id', 'bigint', { nullable: false, ordinal: 1 }), col('code', 'varchar(20)', { ordinal: 2 }), col('amount', 'numeric(10,2)', { ordinal: 3 })],
  primary_key: { name: 'pk_orders', columns: ['id'] },
  uniques: [{ name: 'uk_code', columns: ['code'] }],
  foreign_keys: [
    { name: 'fk_parent', columns: ['code'], ref_schema: null, ref_table: 'parent', ref_columns: ['code'], on_delete: null, on_update: null },
  ],
  checks: [{ name: 'ck_amount', expression: 'amount >= 0' }],
  indexes: [{ name: 'ix_amount', columns: [{ name: 'amount', descending: false }], unique: false, method: 'BTREE' }],
  triggers: [],
  partition: null,
})

describe('类型的拆分与拼接', () => {
  it('长度、精度、单位和后缀', () => {
    expect(splitType('NUMBER(10,2)')).toEqual({ name: 'NUMBER', length: '10', scale: '2', unit: '' })
    expect(splitType('VARCHAR2(20 CHAR)')).toEqual({ name: 'VARCHAR2', length: '20', scale: '', unit: 'CHAR' })
    expect(splitType('timestamp(6) with time zone')).toEqual({
      name: 'timestamp with time zone',
      length: '6',
      scale: '',
      unit: '',
    })
    expect(splitType('character varying(20)').name).toBe('character varying')
    expect(splitType("enum('a','b')")).toEqual({ name: "enum('a','b')", length: '', scale: '', unit: '' })
    expect(splitType(' text ')).toEqual({ name: 'text', length: '', scale: '', unit: '' })
  })

  it('拼回原生类型', () => {
    expect(joinType({ name: 'NUMBER', length: '10', scale: '2', unit: '' })).toBe('NUMBER(10,2)')
    expect(joinType({ name: 'VARCHAR2', length: '50', scale: '', unit: 'CHAR' })).toBe('VARCHAR2(50 CHAR)')
    expect(joinType({ name: 'TIMESTAMP WITH TIME ZONE', length: '6', scale: '', unit: '' })).toBe('TIMESTAMP(6) WITH TIME ZONE')
    expect(joinType({ name: 'INT UNSIGNED', length: '10', scale: '', unit: '' })).toBe('INT(10) UNSIGNED')
    expect(joinType({ name: 'text', length: '', scale: '3', unit: '' })).toBe('text')
    for (const t of ['numeric(12,4)', 'VARCHAR2(20 BYTE)', 'timestamp(3) without time zone', 'jsonb']) {
      expect(joinType(splitType(t))).toBe(t)
    }
  })

  it('按类型目录决定可填的参数', () => {
    const types: TypeEntry[] = [
      { name: 'VARCHAR2', params: 'length', char_unit: true },
      { name: 'DATE', params: 'none', char_unit: false },
    ]
    expect(typeParams('varchar2', types)).toBe('length')
    expect(typeParams('DATE', types)).toBe('none')
    expect(typeParams('my_domain', types)).toBe('precision_scale')
    expect(typeParams("enum('a')", types)).toBe('none')
  })
})

describe('草稿', () => {
  it('打开已有表后没有改动，整理回去与原表一致', () => {
    const d = fromTable(table())
    expect(isDirty(d, fromTable(table()))).toBe(false)
    const m = toModel(d)
    expect(m.columns.map((c) => c.ordinal)).toEqual([1, 2, 3])
    expect(m).toEqual({ ...table(), columns: m.columns })
    expect(renamesOf(d)).toEqual({ columns: {}, indexes: {} })
  })

  it('列改名时引用它的地方跟着改，并记下原名', () => {
    const original = fromTable(table())
    const key = original.columns[1].key
    const d = updateColumn(original, key, { name: 'order_code' })
    expect(d.uniques[0].columns).toEqual(['order_code'])
    expect(d.foreignKeys[0].columns).toEqual(['order_code'])
    expect(d.foreignKeys[0].ref_columns, '引用别的表的列不变').toEqual(['code'])
    expect(renamesOf(d)).toEqual({ columns: { order_code: 'code' }, indexes: {} })
    expect(isDirty(d, original)).toBe(true)
    expect(changedFields(d.columns[1], original)).toEqual(new Set(['name']))
    // 还原：名字和引用都改回
    const back = restoreColumn(d, key, original)
    expect(back.uniques[0].columns).toEqual(['code'])
    expect(isDirty(back, original)).toBe(false)
  })

  it('删除：新增的直接去掉；原有的标记删除，整理时从约束和索引中去掉', () => {
    const original = fromTable(table())
    let d = addColumn(original, { ...emptyColumn('note'), data_type: { native: 'text', logical: {} } }, original.columns[0].key)
    expect(d.columns.map((c) => c.name)).toEqual(['id', 'note', 'code', 'amount'])
    expect(columnState(d.columns[1], original)).toBe('new')
    d = removeColumn(d, d.columns[1].key)
    expect(d.columns).toHaveLength(3)
    d = removeColumn(d, d.columns[1].key)
    expect(columnState(d.columns[1], original)).toBe('deleted')
    const m = toModel(d)
    expect(m.columns.map((c) => c.name)).toEqual(['id', 'amount'])
    expect(m.uniques).toEqual([])
    expect(m.foreign_keys, '引用被删列的外键整个去掉').toEqual([])
    expect(restoreColumn(d, d.columns[1].key, original).columns[1].deleted).toBeUndefined()
  })

  it('调整顺序、主键', () => {
    const original = fromTable(table())
    let d = moveColumn(original, original.columns[2].key, -1)
    expect(d.columns.map((c) => c.name)).toEqual(['id', 'amount', 'code'])
    expect(moveColumn(d, d.columns[0].key, -1)).toBe(d)
    d = togglePrimaryKey(d, 'code')
    expect(d.primaryKey).toEqual({ name: 'pk_orders', columns: ['id', 'code'] })
    expect(d.columns.find((c) => c.name === 'code')?.nullable).toBe(false)
    d = togglePrimaryKey(togglePrimaryKey(d, 'code'), 'id')
    expect(d.primaryKey).toBeNull()
  })

  it('新建表：有名字或有列就算改动', () => {
    expect(isDirty(emptyDraft(), null)).toBe(false)
    expect(isDirty({ ...emptyDraft(), name: 't' }, null)).toBe(true)
    expect(isDirty(addColumn(emptyDraft(), emptyColumn()), null)).toBe(true)
  })
})
