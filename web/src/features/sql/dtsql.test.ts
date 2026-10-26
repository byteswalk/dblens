import { describe, expect, it } from 'vitest'

import { caretAt } from './dtsql'
import { mysql } from './mysql'
import { postgres } from './postgres'
import type { SqlParser } from './types'

/** `|` 表示光标。 */
const at = (p: SqlParser, text: string) => p.suggest(text.replace('|', ''), text.indexOf('|'))

describe('dt-sql-parser 适配', () => {
  it('偏移换算成行列', () => {
    expect(caretAt('SELECT 1;\nSELECT ', 17)).toEqual({ lineNumber: 2, column: 8 })
  })

  it('语法错误按语言给出文字', () => {
    const [zh] = mysql.validate('SELECT * FORM t', 'zh_CN')
    expect(zh).toMatchObject({ startLine: 1, startColumn: 10, endLine: 1, endColumn: 14 })
    expect(zh.message).toContain('FORM')
    expect(zh.message).toMatch(/[一-鿿]/)
    const [en] = postgres.validate('SELECT * FROM t WHERE', 'en_US')
    expect(en.message).toBe('Statement is incomplete')
  })

  it('正确的语句没有问题', () => {
    expect(mysql.validate('SELECT e.id FROM emp e JOIN dept d ON d.id = e.dept_id LIMIT 10;\nUPDATE emp SET a = 1', 'zh_CN')).toEqual([])
    expect(postgres.validate('INSERT INTO t (a) VALUES (1) RETURNING id', 'zh_CN')).toEqual([])
  })

  it('FROM 后需要表', () => {
    expect(at(mysql, 'SELECT * FROM |').want).toEqual(expect.arrayContaining(['table', 'view']))
    expect(at(postgres, 'SELECT * FROM |').want).toEqual(expect.arrayContaining(['table', 'view']))
  })

  it('schema 限定', () => {
    const s = at(mysql, 'SELECT * FROM db1.|')
    expect(s.want).toContain('table')
    expect(s.qualifier).toEqual(['db1'])
  })

  it('别名限定的列，语句中的表和别名', () => {
    const s = at(mysql, 'SELECT a.| FROM emp a JOIN dept d ON a.did = d.id')
    expect(s.want).toContain('column')
    expect(s.qualifier).toEqual(['a'])
    expect(s.tables).toEqual([
      { name: 'emp', alias: 'a' },
      { name: 'dept', alias: 'd' },
    ])
    expect(at(postgres, 'SELECT e.| FROM hr.emp e').tables).toEqual([{ schema: 'hr', name: 'emp', alias: 'e' }])
  })

  it('正在输入的词不算限定名', () => {
    expect(at(mysql, 'SELECT * FROM emp e WHERE e.na|').qualifier).toEqual(['e'])
    expect(at(mysql, 'SELECT na| FROM emp').qualifier).toEqual([])
  })

  it('只看光标所在的语句', () => {
    const s = at(mysql, 'SELECT * FROM a1 x;\nSELECT * FROM emp WHERE id = 1 AND |;\nSELECT 2')
    expect(s.want).toContain('column')
  })

  it('反引号名称去掉引号', () => {
    expect(at(mysql, 'SELECT t.| FROM `my db`.`t 1` t').tables).toEqual([{ schema: 'my db', name: 't 1', alias: 't' }])
  })
})
