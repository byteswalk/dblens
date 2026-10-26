import { describe, expect, it } from 'vitest'

import { type ExecState, type ServerEvent, cellText, initialExec, reduceExec } from './exec'

function run(events: ServerEvent[], maxRows = 2): ExecState {
  return events.reduce((s, e) => reduceExec(s, e, maxRows), initialExec)
}

const start = (index: number, keyword = 'SELECT'): ServerEvent => ({
  type: 'statement_start',
  index,
  start: 0,
  end: 8,
  info: { keyword },
})
const col = { name: 'a', type_name: 'INT', nullable: true }

describe('reduceExec', () => {
  it('一批装满时等 fetch_more，不满或结束时不等', () => {
    let s = run([start(0), { type: 'result_start', columns: [col] }, { type: 'rows', rows: [[{ t: 'int', v: 1 }], [{ t: 'int', v: 2 }]] }])
    expect(s.waitingForMore).toBe(true)
    s = reduceExec(s, { type: 'rows', rows: [[{ t: 'int', v: 3 }]] }, 2)
    expect(s.waitingForMore).toBe(false)
    s = reduceExec(s, { type: 'result_end' }, 2)
    const r = s.statements[0].results[0]
    expect(r.rows.map((row) => row[0].v)).toEqual([1, 2, 3])
    expect(r.complete).toBe(true)
  })

  it('多条语句各自记录结果、影响行数、消息和耗时', () => {
    const s = run([
      start(0, 'UPDATE'),
      { type: 'affected', count: 3 },
      { type: 'message', text: 'notice' },
      { type: 'statement_end', index: 0, elapsed_ms: 12 },
      start(1),
      { type: 'result_start', columns: [col] },
      { type: 'result_end' },
      { type: 'statement_end', index: 1, elapsed_ms: 5 },
      { type: 'done' },
    ])
    expect(s.statements.map((x) => [x.keyword, x.affected, x.messages, x.elapsedMs, x.results.length])).toEqual([
      ['UPDATE', 3, ['notice'], 12, 0],
      ['SELECT', null, [], 5, 1],
    ])
    expect(s.done).toBe(true)
  })

  it('失败、需要确认和协议错误', () => {
    const failed = run([start(0), { type: 'failed', index: 0, error: { kind: 'cancelled' } }, { type: 'done' }])
    expect(failed.statements[0].error).toEqual({ kind: 'cancelled' })
    const confirm = run([start(0, 'DELETE'), { type: 'confirm_required', index: 0, start: 10, reason: '没有 WHERE' }])
    expect(confirm.statements[0].confirm).toEqual({ start: 10, reason: '没有 WHERE' })
    const proto = run([{ type: 'error', error: { kind: 'busy', message: '上一段还没执行完' } }])
    expect(proto.protocolError?.kind).toBe('busy')
  })
})

describe('cellText', () => {
  it('各类值的显示文本', () => {
    expect(cellText({ t: 'null' })).toBe('NULL')
    expect(cellText({ t: 'int', v: 5 })).toBe('5')
    expect(cellText({ t: 'decimal', v: '1.50' })).toBe('1.50')
    expect(cellText({ t: 'json', v: { a: 1 } })).toBe('{"a":1}')
    expect(cellText({ t: 'other', v: { type_name: 'geometry', text: 'POINT(1 2)' } })).toBe('POINT(1 2)')
    expect(cellText({ t: 'lob', v: { binary: false, preview: 'abc', total_len: 100, truncated: true } })).toBe(
      'abc…（共 100）',
    )
  })
})
