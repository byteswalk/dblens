// 结果面板的内容（M1 设计文档 1 节）：每个结果集一个标签，失败的语句单独一个标签，最后是"消息"。
// 纯函数，按执行状态生成，单独测试。

import { errorText } from '../../shared/api'
import type { ExecState, StatementState } from '../../shared/exec'
import { t } from '../../shared/i18n'

export type ResultTab =
  | { key: string; kind: 'result'; label: string; stmt: number; result: number; rows: number; capped: boolean }
  | { key: string; kind: 'error'; label: string; stmt: number }
  | { key: 'messages'; kind: 'messages'; label: string }

/** 因达到行数上限被取消的语句不算失败。 */
function failed(s: StatementState, i: number, capped: number | null): boolean {
  return s.error !== null && !(i === capped && s.error.kind === 'cancelled')
}

export function resultTabs(exec: ExecState, explain: boolean, capped: number | null): ResultTab[] {
  const tabs: ResultTab[] = []
  let n = 0
  exec.statements.forEach((s, i) => {
    s.results.forEach((r, j) => {
      n += 1
      tabs.push({
        key: `r${i}.${j}`,
        kind: 'result',
        label: explain ? t('results.tab.explain') : t('results.tab.result', { n }),
        stmt: i,
        result: j,
        rows: r.rows.length,
        capped: i === capped,
      })
    })
    if (failed(s, i, capped)) {
      const label = t(s.error?.kind === 'cancelled' ? 'results.tab.cancelled' : 'results.tab.failed', { n: i + 1 })
      tabs.push({ key: `e${i}`, kind: 'error', label, stmt: i })
    }
  })
  tabs.push({ key: 'messages', kind: 'messages', label: t('results.tab.messages') })
  return tabs
}

export interface MessageLine {
  level: 'info' | 'warn' | 'error'
  text: string
}

/** "消息"标签的内容：每条语句的结果摘要、服务器消息、错误，以及确认（`declined`：用户取消了）和协议错误。 */
export function messageLines(exec: ExecState, capped: number | null, declined = false): MessageLine[] {
  const out: MessageLine[] = []
  exec.statements.forEach((s, i) => {
    const n = i + 1
    const ms = s.elapsedMs
    for (const text of s.messages) out.push({ level: 'info', text: t('results.message.server', { n, text }) })
    s.results.forEach((r) => {
      const count = r.rows.length
      const text =
        ms === null ? t('results.message.rows', { n, count }) : t('results.message.rowsElapsed', { n, count, ms })
      out.push({ level: 'info', text })
    })
    if (i === capped) {
      const count = s.results.at(-1)?.rows.length ?? 0
      out.push({ level: 'warn', text: t(s.last ? 'results.message.capped' : 'results.message.cappedMore', { n, count }) })
    } else if (s.error) {
      out.push({ level: 'error', text: t('results.message.error', { n, error: errorText(s.error) }) })
    } else if (s.affected !== null && s.results.length === 0) {
      // 有结果集的语句（如 Oracle 的执行计划先写计划表）不再报影响行数
      const count = s.affected
      const text =
        ms === null
          ? t('results.message.affected', { n, count })
          : t('results.message.affectedElapsed', { n, count, ms })
      out.push({ level: 'info', text })
    } else if (s.results.length === 0 && ms !== null) {
      out.push({ level: 'info', text: t('results.message.succeededElapsed', { n, ms }) })
    }
  })
  if (exec.confirm) {
    const n = exec.statements.length + 1
    const reason = exec.confirm.reason
    out.push({
      level: 'warn',
      text: t(declined ? 'results.message.declined' : 'results.message.confirmRequired', { n, reason }),
    })
  }
  if (exec.protocolError) out.push({ level: 'error', text: errorText(exec.protocolError) })
  return out
}

/** 状态栏上最近一次执行的摘要：最后一个结果集的行数，或最后一条语句的影响行数，加上耗时。 */
export function lastSummary(exec: ExecState): string | null {
  const s = exec.statements.at(-1)
  if (!s || s.elapsedMs === null) return null
  const r = s.results.at(-1)
  if (r) return t('results.summary.rows', { count: r.rows.length, ms: s.elapsedMs })
  if (s.affected !== null) return t('results.summary.affected', { count: s.affected, ms: s.elapsedMs })
  return `${s.elapsedMs} ms`
}

/** 表格滚动到离已加载的末尾不到 `margin` 行、且服务端在等时，续取下一批。 */
export function shouldFetchMore(lastVisibleRow: number, loaded: number, waiting: boolean, margin = 100): boolean {
  return waiting && lastVisibleRow >= loaded - margin
}
