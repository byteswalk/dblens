// 历史侧栏的纯逻辑（M1c1 设计文档第 6 节）：按日期分组、SQL 摘要、说明文字。

import type { HistoryItem } from '../../shared/api'
import { t } from '../../shared/i18n'

/** 每页条数。 */
export const PAGE = 100

export interface DayGroup {
  /** 当天 0 点（本地时间）的时间戳，用作 key。 */
  day: number
  label: string
  items: HistoryItem[]
}

function startOfDay(ms: number): number {
  const d = new Date(ms)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

/** 按本地日期分组：今天、昨天，更早的显示日期。列表本身已按时间倒序。 */
export function groupByDay(items: HistoryItem[], now = Date.now()): DayGroup[] {
  const today = startOfDay(now)
  const yesterday = startOfDay(today - 1)
  const out: DayGroup[] = []
  for (const item of items) {
    const day = startOfDay(item.at)
    let g = out[out.length - 1]
    if (!g || g.day !== day) {
      const label =
        day === today
          ? t('app.history.today')
          : day === yesterday
            ? t('app.history.yesterday')
            : new Date(day).toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })
      g = { day, label, items: [] }
      out.push(g)
    }
    g.items.push(item)
  }
  return out
}

/** SQL 的摘要：第一行非空内容，连续空白合成一个。 */
export function firstLine(sql: string): string {
  const line = sql.split('\n').find((l) => l.trim()) ?? ''
  return line.trim().replace(/\s+/g, ' ')
}

/** 时间（时:分:秒）、耗时、行数。 */
export function detail(item: HistoryItem): string {
  const time = new Date(item.at).toLocaleTimeString(undefined, { hour12: false })
  const parts = [time]
  if (item.elapsed_ms !== null) parts.push(t('app.history.elapsed', { ms: item.elapsed_ms }))
  if (item.rows !== null && !item.error) parts.push(t('app.history.rows', { count: item.rows }))
  return parts.join(' · ')
}
