import { describe, expect, it } from 'vitest'

import { syntaxMarks } from './diagnostics'

const issue = { startLine: 2, startColumn: 5, endLine: 2, endColumn: 9 }

describe('syntaxMarks', () => {
  it('标准档：错误，文字用解析器给的', () => {
    const [m] = syntaxMarks([{ ...issue, message: "'FORM' 在此位置无效" }], { dialect: 'mysql', strict: true, grammar: 'MySQL' }, 'MySQL')
    expect(m).toEqual({
      severity: 'error',
      message: "'FORM' 在此位置无效",
      startLineNumber: 2,
      startColumn: 5,
      endLineNumber: 2,
      endColumn: 9,
    })
  })

  it('兼容档：警告，说明按哪种语法检查', () => {
    const [m] = syntaxMarks([{ ...issue, near: 'LIMIT' }], { dialect: 'plsql', strict: false, grammar: 'Oracle' }, '达梦')
    expect(m.severity).toBe('warning')
    expect(m.message).toBe('“LIMIT”附近有语法错误（按 Oracle 语法检查，达梦 的扩展语法可能被误报）')
  })

  it('没有文字时按不完整 / 出错的词生成；至少标一个字符', () => {
    const [m] = syntaxMarks([{ ...issue, endColumn: 5, incomplete: true }], { dialect: 'plsql', strict: true, grammar: 'Oracle' }, 'Oracle')
    expect(m.message).toBe('语句不完整')
    expect(m.endColumn).toBe(6)
  })
})
