import { describe, expect, it } from 'vitest'

import { type ExecState, type StatementState, initialExec } from '../../shared/exec'
import { errorMarks } from './markers'

function stmt(p: Partial<StatementState>): StatementState {
  return {
    index: 0,
    last: true,
    start: 0,
    end: 0,
    keyword: 'SELECT',
    results: [],
    affected: null,
    messages: [],
    elapsedMs: null,
    error: null,
    errorOffset: null,
    ...p,
  }
}

describe('错误标记', () => {
  const text = "select 1;\nselect '数洞', from t;\nupdate x set a = 1"
  const second = text.indexOf("select '")
  const err = { kind: 'sql', code: '42601', message: 'syntax error' }

  it('有出错位置时标出那个词，否则标整条语句；取消不标', () => {
    const exec: ExecState = {
      ...initialExec,
      statements: [
        stmt({ start: 0, end: 8 }),
        stmt({ start: second, end: second + 21, error: err, errorOffset: text.indexOf('from') }),
        stmt({ start: text.indexOf('update'), end: text.length, error: err }),
        stmt({ start: 0, end: 8, error: { kind: 'cancelled' } }),
      ],
    }
    const marks = errorMarks(exec, text)
    expect(marks.map((m) => text.slice(m.start, m.end))).toEqual(['from', 'update x set a = 1'])
    expect(marks[0].message).toBe('SQL 错误 [42601]：syntax error')
  })

  it('位置落在符号上时标一个字符', () => {
    const exec = { ...initialExec, statements: [stmt({ error: err, errorOffset: text.indexOf(';') })] }
    expect(errorMarks(exec, text).map((m) => m.end - m.start)).toEqual([1])
  })
})
