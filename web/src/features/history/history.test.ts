import { describe, expect, it } from 'vitest'

import type { HistoryItem } from '../../shared/api'
import { detail, firstLine, groupByDay } from './history'

function item(id: number, at: number, extra: Partial<HistoryItem> = {}): HistoryItem {
  return {
    id,
    at,
    profile_id: 'p1',
    profile_name: '开发库',
    driver: 'mysql',
    variant: null,
    schema: null,
    sql: `SELECT ${id}`,
    elapsed_ms: 12,
    rows: 3,
    error: null,
    ...extra,
  }
}

describe('groupByDay', () => {
  it('今天、昨天，更早的按日期', () => {
    const now = new Date(2026, 9, 26, 15, 0).getTime()
    const items = [
      item(4, new Date(2026, 9, 26, 9, 0).getTime()),
      item(3, new Date(2026, 9, 26, 0, 0).getTime()),
      item(2, new Date(2026, 9, 25, 23, 59).getTime()),
      item(1, new Date(2026, 9, 20, 8, 0).getTime()),
    ]
    const groups = groupByDay(items, now)
    expect(groups.map((g) => g.label)).toEqual(['今天', '昨天', new Date(2026, 9, 20).toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })])
    expect(groups.map((g) => g.items.map((i) => i.id))).toEqual([[4, 3], [2], [1]])
  })

  it('没有历史时为空', () => {
    expect(groupByDay([])).toEqual([])
  })
})

describe('firstLine / detail', () => {
  it('取第一行非空内容，合并空白', () => {
    expect(firstLine('\n\n  SELECT   a,\n  b FROM t')).toBe('SELECT a,')
    expect(firstLine('   ')).toBe('')
  })

  it('耗时和行数；失败的不显示行数', () => {
    const at = new Date(2026, 9, 26, 9, 5, 7).getTime()
    const time = new Date(at).toLocaleTimeString(undefined, { hour12: false })
    expect(detail(item(1, at))).toBe(`${time} · 12 ms · 3 行`)
    expect(detail(item(1, at, { error: '已取消', rows: 0 }))).toBe(`${time} · 12 ms`)
    expect(detail(item(1, at, { elapsed_ms: null, rows: null }))).toBe(time)
  })
})
