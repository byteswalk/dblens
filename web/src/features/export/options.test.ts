import { describe, expect, it } from 'vitest'

import { baseName, defaultFileName, defaultOptions, fileFilter } from './options'

describe('导出选项', () => {
  it('默认选项：逗号分隔、带表头和 BOM，NULL 写成空，INSERT 每条一行', () => {
    expect(defaultOptions()).toEqual({
      delimiter: ',',
      header: true,
      bom: true,
      null_text: '',
      table: 'table_name',
      rows_per_insert: 1,
    })
    expect(defaultOptions('public.orders').table).toBe('public.orders')
  })

  it('默认文件名：对象名加时间，去掉不能用的字符', () => {
    const now = new Date(2026, 9, 26, 9, 5, 3)
    expect(defaultFileName('orders', 'csv', now)).toBe('orders_20261026_090503.csv')
    expect(defaultFileName('订单 明细', 'insert', now)).toBe('订单_明细_20261026_090503.sql')
    expect(defaultFileName('a/b:c', 'xlsx', now)).toBe('a_b_c_20261026_090503.xlsx')
    expect(defaultFileName('', 'json', now)).toBe('export_20261026_090503.json')
  })

  it('另存为的文件类型和路径中的文件名', () => {
    expect(fileFilter('insert', 'INSERT')).toEqual({ name: 'INSERT', extensions: ['sql'] })
    expect(baseName('D:\\导出\\a.csv')).toBe('a.csv')
    expect(baseName('/home/u/a.json')).toBe('a.json')
  })
})
