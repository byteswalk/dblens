import { describe, expect, it } from 'vitest'

import { completionItems, lightSuggest, quoteIdent } from './completion'
import type { MetaReader } from './metadata'
import type { Suggest } from './types'

const meta: MetaReader = {
  schemas: async () => ['HR', 'SALES'],
  objects: async (s) =>
    s === 'HR'
      ? [
          { name: 'EMP', kind: 'table', comment: '员工' },
          { name: 'DEPT', kind: 'table' },
          { name: 'V_EMP', kind: 'view' },
          { name: 'EMP_SEQ', kind: 'sequence' },
        ]
      : [{ name: 'ORDERS', kind: 'table' }],
  columns: async (s, t) =>
    s === 'HR' && t === 'EMP'
      ? [
          { name: 'ID', type: 'NUMBER' },
          { name: 'NAME', type: 'VARCHAR2(50)', comment: '姓名' },
        ]
      : s === 'HR' && t === 'DEPT'
        ? [
            { name: 'ID', type: 'NUMBER' },
            { name: 'TITLE', type: 'VARCHAR2(20)' },
          ]
        : [],
  functions: async (s) => (s === 'HR' ? ['F_BONUS'] : []),
}

const sug = (p: Partial<Suggest>): Suggest => ({ keywords: [], want: [], qualifier: [], tables: [], ...p })
const run = (p: Partial<Suggest>, schema: string | null = 'HR') => completionItems({ suggest: sug(p), dialect: 'plsql', schema, meta })
const labels = async (p: Partial<Suggest>, kind?: string) => (await run(p)).filter((i) => !kind || i.kind === kind).map((i) => i.label)

describe('completionItems', () => {
  it('需要表时给出当前 schema 的表、视图和所有 schema', async () => {
    const items = await run({ want: ['table', 'view'] })
    expect(items.map((i) => [i.kind, i.label])).toEqual([
      ['table', 'EMP'],
      ['table', 'DEPT'],
      ['view', 'V_EMP'],
      ['schema', 'HR'],
      ['schema', 'SALES'],
    ])
    expect(items[0].detail).toBe('员工')
  })

  it('schema 限定时给出该 schema 的对象', async () => {
    expect(await labels({ want: ['table'], qualifier: ['sales'] })).toEqual(['ORDERS'])
  })

  it('别名限定时只给该表的列，语句中的名称不分大小写', async () => {
    const items = await run({ want: ['column', 'function'], qualifier: ['e'], tables: [{ name: 'emp', alias: 'e' }, { name: 'dept', alias: 'd' }] })
    expect(items.map((i) => i.label)).toEqual(['ID', 'NAME'])
    expect(items[1].detail).toBe('VARCHAR2(50) · 姓名')
  })

  it('不限定时给出语句中各表的列（同名列合并说明）、函数和序列', async () => {
    const items = await run({
      want: ['column', 'function'],
      keywords: ['CASE'],
      tables: [{ schema: 'hr', name: 'emp', alias: 'e' }, { name: 'DEPT' }],
    })
    const cols = items.filter((i) => i.kind === 'column')
    expect(cols.map((i) => i.label)).toEqual(['ID', 'NAME', 'TITLE'])
    expect(cols[0].detail).toBe('NUMBER · e; NUMBER · DEPT')
    expect(items.some((i) => i.kind === 'function' && i.label === 'NVL')).toBe(true)
    expect(items.some((i) => i.kind === 'function' && i.label === 'F_BONUS')).toBe(true)
    expect(items.some((i) => i.kind === 'keyword' && i.label === 'CASE')).toBe(true)
  })

  it('schema.表. 给出列；语句中没写的当前 schema 表名也能给列', async () => {
    expect(await labels({ want: ['column'], qualifier: ['HR', 'DEPT'] })).toEqual(['ID', 'TITLE'])
    expect(await labels({ want: ['column'], qualifier: ['emp'] })).toEqual(['ID', 'NAME'])
  })

  it('排序：列（按表中的顺序）、表、schema、函数、关键字', async () => {
    const items = await run({ want: ['column', 'table', 'function'], keywords: ['WHERE'], tables: [{ name: 'EMP' }, { name: 'DEPT' }] })
    const sorted = [...items].sort((a, b) => (a.sortText < b.sortText ? -1 : 1))
    expect(sorted.map((i) => i.kind).filter((k, i, a) => a.indexOf(k) === i)).toEqual(['column', 'table', 'view', 'sequence', 'schema', 'function', 'keyword'])
    expect(sorted.filter((i) => i.kind === 'column').map((i) => i.label)).toEqual(['ID', 'NAME', 'TITLE'])
  })

  it('当前 schema 未知时不给表，仍给 schema', async () => {
    expect(await completionItems({ suggest: sug({ want: ['table'] }), dialect: 'plsql', schema: null, meta }).then((x) => x.map((i) => i.kind))).toEqual([
      'schema',
      'schema',
    ])
  })
})

describe('quoteIdent', () => {
  it.each([
    ['EMP', 'plsql', 'EMP'],
    ['Emp', 'plsql', '"Emp"'],
    ['emp', 'postgres', 'emp'],
    ['Emp', 'postgres', '"Emp"'],
    ['my table', 'mysql', '`my table`'],
    ['Emp', 'mysql', 'Emp'],
    ['数洞表', 'plsql', '"数洞表"'],
  ] as const)('%s（%s）→ %s', (name, d, out) => {
    expect(quoteIdent(name, d)).toBe(out)
  })
})

describe('lightSuggest', () => {
  it('取限定名和语句中的表', () => {
    const text = 'SELECT e. FROM hr.emp e JOIN dept AS d ON 1 = 1 WHERE x'
    const s = lightSuggest(text, 'SELECT e.'.length)
    expect(s.qualifier).toEqual(['e'])
    expect(s.tables).toEqual([{ schema: 'hr', name: 'emp', alias: 'e' }, { name: 'dept', alias: 'd' }])
    expect(lightSuggest('SELECT * FROM t WHERE', 21).tables).toEqual([{ name: 't' }])
  })
})
