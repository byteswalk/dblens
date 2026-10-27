import { describe, expect, it } from 'vitest'

import { editableKind, parseCell, parseTsv } from './parse'

const v = (text: string, kind: string, extra = {}) => parseCell(text, { kind, ...extra })

describe('按类型解析单元格', () => {
  it('空文本：文本类为空字符串，其他为 NULL', () => {
    expect(v('', 'var_char')).toEqual({ value: { t: 'string', v: '' } })
    expect(v('', 'int', { bits: 32 })).toEqual({ value: { t: 'null' } })
    expect(v('', 'date')).toEqual({ value: { t: 'null' } })
  })

  it('布尔、整数（范围）、浮点、精确小数', () => {
    expect(v('TRUE', 'bool')).toEqual({ value: { t: 'bool', v: true } })
    expect(v('0', 'bool')).toEqual({ value: { t: 'bool', v: false } })
    expect(v('是', 'bool')).toHaveProperty('error')
    expect(v(' 42 ', 'int', { bits: 32 })).toEqual({ value: { t: 'int', v: 42 } })
    expect(v('32768', 'int', { bits: 16 })).toHaveProperty('error')
    expect(v('-1', 'u_int', { bits: 8 })).toHaveProperty('error')
    expect(v('9223372036854775807', 'int', { bits: 64 })).toEqual({ value: { t: 'decimal', v: '9223372036854775807' } })
    expect(v('1.5', 'int', { bits: 32 })).toHaveProperty('error')
    expect(v('1e3', 'float', { bits: 64 })).toEqual({ value: { t: 'float', v: 1000 } })
    expect(v('12.30', 'decimal')).toEqual({ value: { t: 'decimal', v: '12.30' } })
    expect(v('12,30', 'decimal')).toHaveProperty('error')
  })

  it('日期、时间、时间戳（带时区时补上时区）', () => {
    expect(v('2026/1/5', 'date')).toEqual({ value: { t: 'date', v: '2026-01-05' } })
    expect(v('2026-02-30', 'date')).toHaveProperty('error')
    expect(v('9:05', 'time')).toEqual({ value: { t: 'time', v: '09:05:00' } })
    expect(v('25:00', 'time')).toHaveProperty('error')
    expect(v('2026-10-26 13:45:30.5', 'timestamp', { tz: false })).toEqual({
      value: { t: 'date_time', v: '2026-10-26T13:45:30.5' },
    })
    expect(v('2026-10-26T13:45 +0800', 'timestamp', { tz: true })).toEqual({
      value: { t: 'date_time_tz', v: '2026-10-26T13:45:00+08:00' },
    })
    const local = v('2026-10-26 13:45', 'timestamp', { tz: true })
    expect('value' in local && String(local.value.v)).toMatch(/^2026-10-26T13:45:00[+-]\d{2}:\d{2}$/)
  })

  it('UUID、JSON、其他按原文', () => {
    expect(v('3F2504E0-4F89-11D3-9A0C-0305E82C3301', 'uuid')).toEqual({
      value: { t: 'uuid', v: '3f2504e0-4f89-11d3-9a0c-0305e82c3301' },
    })
    expect(v('{"a": 1}', 'json')).toEqual({ value: { t: 'json', v: { a: 1 } } })
    expect(v('{a', 'json')).toHaveProperty('error')
    expect(v('中文 ', 'text')).toEqual({ value: { t: 'string', v: '中文 ' } })
    expect(editableKind({ kind: 'blob' })).toBe(false)
    expect(editableKind({ kind: 'text' })).toBe(true)
  })
})

describe('解析制表符分隔的文本', () => {
  it('行、列，末尾空行忽略，Windows 换行', () => {
    expect(parseTsv('a\tb\r\n1\t2\r\n')).toEqual([
      ['a', 'b'],
      ['1', '2'],
    ])
    expect(parseTsv('only')).toEqual([['only']])
    expect(parseTsv('a\t\tc')).toEqual([['a', '', 'c']])
  })

  it('引号包住的单元格可以含换行、制表符和引号', () => {
    expect(parseTsv('"多\n行"\t"x""y"\t"a\tb"\n2\t3\t4')).toEqual([
      ['多\n行', 'x"y', 'a\tb'],
      ['2', '3', '4'],
    ])
  })
})
