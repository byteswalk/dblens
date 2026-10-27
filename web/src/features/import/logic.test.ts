import { describe, expect, it } from 'vitest'

import type { ColumnInfo, TableInfo } from '../../shared/api'
import {
  autoMap,
  cellError,
  draftOf,
  draftProblem,
  fileFormat,
  fileNameOf,
  logicalOf,
  modeAllowed,
  newTable,
  sizeText,
  tableNameFromFile,
  upsertBlocked,
} from './logic'

const col = (name: string, logical: unknown): ColumnInfo => ({
  name,
  ordinal: 1,
  data_type: { native: 'x', logical },
  nullable: true,
  default: null,
  comment: null,
})

describe('导入向导', () => {
  it('按扩展名判断格式，取路径中的文件名', () => {
    expect(fileFormat('订单.CSV')).toBe('csv')
    expect(fileFormat('a.tsv')).toBe('csv')
    expect(fileFormat('b.xlsx')).toBe('excel')
    expect(fileFormat('c.xls')).toBe('excel')
    expect(fileFormat('d.json')).toBeNull()
    expect(fileNameOf('C:\\data\\订单.csv')).toBe('订单.csv')
    expect(fileNameOf('/home/u/a.xlsx')).toBe('a.xlsx')
  })

  it('自动映射：同名（不区分大小写、去空白），一个表列只用一次', () => {
    expect(autoMap([' ID ', 'Name', '备注', 'name'], ['id', 'name', 'price'])).toEqual(['id', 'name', null, null])
  })

  it('新建表的默认表名', () => {
    expect(tableNameFromFile('2026 订单-明细.csv')).toBe('t_2026_订单_明细')
    expect(tableNameFromFile('orders.xlsx')).toBe('orders')
    expect(tableNameFromFile('---.csv')).toBe('import')
  })

  it('按主键更新的条件；新建表只能追加', () => {
    const t = (pk: string[] | null): TableInfo => ({
      name: 't',
      comment: null,
      columns: [],
      primary_key: pk ? { name: null, columns: pk } : null,
    })
    expect(upsertBlocked(t(null), ['id'])).toBe('no_key')
    expect(upsertBlocked(t(['id', 'day']), ['id'])).toBe('key_unmapped')
    expect(upsertBlocked(t(['id']), ['id', 'name'])).toBeNull()
    expect(modeAllowed('upsert', false, 'no_key')).toBe(false)
    expect(modeAllowed('upsert', false, null)).toBe(true)
    expect(modeAllowed('replace', true, null)).toBe(false)
    expect(modeAllowed('append', true, null)).toBe(true)
  })

  it('预览单元格按目标列的类型校验，只查文本', () => {
    const int = col('qty', { kind: 'int', bits: 32 })
    expect(cellError({ t: 'string', v: '12' }, int)).toBeNull()
    expect(cellError({ t: 'string', v: 'abc' }, int)).not.toBeNull()
    expect(cellError({ t: 'float', v: 1.5 }, int)).toBeNull()
    expect(cellError({ t: 'null' }, int)).toBeNull()
    expect(cellError({ t: 'string', v: '2026/1/2' }, col('d', { kind: 'date' }))).toBeNull()
  })

  it('推断的类型与界面选项互转', () => {
    expect(draftOf('a', { kind: 'int', bits: 64 }).type).toBe('bigint')
    const dec = draftOf('b', { kind: 'decimal', precision: 12, scale: 3 })
    expect([dec.type, dec.precision, dec.scale]).toEqual(['decimal', 12, 3])
    expect(logicalOf(dec)).toEqual({ kind: 'decimal', precision: 12, scale: 3 })
    const s = draftOf('c', { kind: 'var_char', len: 80 })
    expect(logicalOf(s)).toEqual({ kind: 'var_char', len: 80 })
    expect(logicalOf(draftOf('d', { kind: 'timestamp', tz: false, precision: null }))).toEqual({
      kind: 'timestamp',
      tz: false,
      precision: null,
    })
    expect(draftOf('e', { kind: 'json' }).type).toBe('var_char')
  })

  it('新建表的定义：主键列不可为空；空名和重名', () => {
    const cols = [
      { ...draftOf(' id ', { kind: 'int', bits: 64 }), key: true },
      draftOf('名称', { kind: 'var_char', len: 50 }),
    ]
    expect(newTable(cols)).toEqual({
      columns: [
        { name: 'id', logical: { kind: 'int', bits: 64 }, nullable: false },
        { name: '名称', logical: { kind: 'var_char', len: 50 }, nullable: true },
      ],
      primary_key: ['id'],
    })
    expect(draftProblem(cols)).toBeNull()
    expect(draftProblem([...cols, draftOf('ID', { kind: 'text' })])).toBe('duplicate')
    expect(draftProblem([draftOf(' ', { kind: 'text' })])).toBe('empty')
  })

  it('文件大小的显示', () => {
    expect(sizeText(100)).toBe('1 KB')
    expect(sizeText(2.5 * 1024 * 1024)).toBe('2.5 MB')
  })
})
