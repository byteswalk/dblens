import { describe, expect, it } from 'vitest'

import type { ColumnInfo, TableInfo } from '../../shared/api'
import { editInfo } from './editable'

const col = (name: string, kind: string, extra: Partial<ColumnInfo> = {}): ColumnInfo => ({
  name,
  ordinal: 0,
  data_type: { native: kind, logical: { kind } },
  nullable: true,
  default: null,
  comment: null,
  ...extra,
})

describe('可编辑的列和行标识', () => {
  it('主键；生成列、二进制和结果集中多出的列不能编辑', () => {
    const table: TableInfo = {
      name: 't',
      comment: null,
      columns: [col('id', 'int'), col('name', 'var_char'), col('total', 'decimal', { generated: 'a * b' }), col('pic', 'blob')],
      primary_key: { name: null, columns: ['id'] },
    }
    const info = editInfo(['id', 'name', 'total', 'pic', 'extra'], table)
    expect(info.key).toEqual([0])
    expect(info.columns.map((c) => c.editable)).toEqual([true, true, false, false, false])
    expect(info.columns[1].logical).toEqual({ kind: 'var_char' })
  })

  it('没有主键时用所有列都非空的唯一键；都没有时为 null', () => {
    const table: TableInfo = {
      name: 't',
      comment: null,
      columns: [col('a', 'int'), col('code', 'var_char', { nullable: false }), col('b', 'int')],
      primary_key: null,
      uniques: [
        { name: null, columns: ['b'] },
        { name: null, columns: ['code'] },
      ],
    }
    expect(editInfo(['a', 'code', 'b'], table).key).toEqual([1])
    expect(editInfo(['a', 'b'], table).key).toBeNull()
    expect(editInfo(['a'], { ...table, uniques: [] }).key).toBeNull()
  })
})
