import { describe, expect, it } from 'vitest'

import { type ExecState, type ServerEvent, initialExec, reduceExec } from '../../shared/exec'
import { lastSummary, messageLines, resultTabs, shouldFetchMore } from './results'

function run(events: ServerEvent[]): ExecState {
  return events.reduce((s, e) => reduceExec(s, e, 500), initialExec)
}

const start = (index: number, keyword = 'SELECT'): ServerEvent => ({
  type: 'statement_start',
  index,
  total: 3,
  start: 0,
  end: 1,
  info: { keyword },
})
const col = { name: 'a', type_name: 'int', nullable: true }
const row = [{ t: 'int', v: 1 }]

const script = run([
  start(0),
  { type: 'result_start', columns: [col] },
  { type: 'rows', rows: [row, row] },
  { type: 'result_end' },
  { type: 'statement_end', index: 0, elapsed_ms: 38 },
  start(1, 'UPDATE'),
  { type: 'failed', index: 1, error: { kind: 'sql', code: '1054', message: "Unknown column 'x'" } },
  start(2, 'UPDATE'),
  { type: 'message', text: 'NOTICE: x' },
  { type: 'affected', count: 3 },
  { type: 'statement_end', index: 2, elapsed_ms: 5 },
  { type: 'done' },
])

describe('结果面板', () => {
  it('每个结果集一个标签，失败的语句一个标签，最后是消息', () => {
    expect(resultTabs(script, false, null).map((t) => t.label)).toEqual(['结果 1', '语句 2 失败', '消息'])
    const first = resultTabs(script, false, null)[0]
    expect(first).toMatchObject({ kind: 'result', stmt: 0, result: 0, rows: 2, capped: false })
    expect(resultTabs(script, true, null)[0].label).toBe('执行计划')
  })

  it('达到行数上限被取消的语句不算失败，其他取消显示已取消', () => {
    const capped = run([
      start(0),
      { type: 'result_start', columns: [col] },
      { type: 'rows', rows: [row] },
      { type: 'failed', index: 0, error: { kind: 'cancelled' } },
      { type: 'done' },
    ])
    expect(resultTabs(capped, false, 0).map((t) => t.label)).toEqual(['结果 1', '消息'])
    expect(resultTabs(capped, false, 0)[0]).toMatchObject({ capped: true })
    expect(resultTabs(capped, false, null).map((t) => t.label)).toEqual(['结果 1', '语句 1 已取消', '消息'])
    expect(messageLines(capped, 0).at(-1)).toEqual({
      level: 'warn',
      text: '[语句 1] 已读取 1 行，达到行数上限，停止读取，后面的语句没有执行',
    })
  })

  it('消息：结果摘要、错误、服务器消息、影响行数', () => {
    expect(messageLines(script, null)).toEqual([
      { level: 'info', text: '[语句 1] 2 行，38 ms' },
      { level: 'error', text: "[语句 2] SQL 错误 [1054]：Unknown column 'x'" },
      { level: 'info', text: '[语句 3] NOTICE: x' },
      { level: 'info', text: '[语句 3] 影响 3 行，5 ms' },
    ])
  })

  it('需要确认时说明原因', () => {
    const s = run([{ type: 'confirm_required', index: 0, start: 0, end: 13, reason: '生产库上不带 WHERE 的 DELETE' }])
    expect(messageLines(s, null)).toEqual([{ level: 'warn', text: '[语句 1] 需要确认：生产库上不带 WHERE 的 DELETE' }])
    expect(messageLines(s, null, true)).toEqual([
      { level: 'warn', text: '[语句 1] 已取消执行（生产库上不带 WHERE 的 DELETE），后面的语句也没有执行' },
    ])
  })

  it('状态栏摘要取最后一条语句', () => {
    expect(lastSummary(script)).toBe('影响 3 行 · 5 ms')
    expect(lastSummary(run([start(0), { type: 'result_start', columns: [col] }]))).toBeNull()
    expect(lastSummary(initialExec)).toBeNull()
  })

  it('滚动到接近末尾且服务端在等时续取', () => {
    expect(shouldFetchMore(450, 500, true)).toBe(true)
    expect(shouldFetchMore(100, 500, true)).toBe(false)
    expect(shouldFetchMore(499, 500, false)).toBe(false)
  })
})
