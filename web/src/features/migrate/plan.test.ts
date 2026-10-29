import { describe, expect, it } from 'vitest'

import type { MigrationPreview } from '../../shared/api'
import { emptyPlan, notesOf, parsePlan, ready, setCheck, setColumn, setExisting, setSequence, setTable, summary, unchosen, worst } from './plan'

const src = { profile: 'a', schema: 's' }

function preview(): MigrationPreview {
  const table = (name: string) => ({
    name,
    comment: null,
    columns: [],
    primary_key: null,
    uniques: [],
    foreign_keys: [],
    checks: [],
    indexes: [],
    triggers: [],
    partition: null,
  })
  return {
    source_variant: 'postgres',
    target_variant: 'oracle',
    source_tables: [],
    source_sequences: [],
    draft: {
      tables: [
        { source: 'a', target: table('A'), columns: [], exists: true },
        { source: 'b', target: table('B'), columns: [], exists: false },
      ],
      sequences: [],
      notes: [
        { kind: 'table', object: 'a', column: 'x', level: 'lossy', message: 'm1' },
        { kind: 'table', object: 'a', column: 'y', level: 'unsupported', message: 'm2' },
        { kind: 'table', object: 'a', level: 'info', message: 'm3' },
      ],
    },
    others: [],
    statements: [
      { phase: 1, kind: 'create_table', object: 'b', requires: [], sql: 'CREATE TABLE B' },
      { phase: 1, kind: 'comment', object: 'b', requires: ['b'], sql: 'COMMENT' },
      { phase: 2, kind: 'index', object: 'b', requires: ['b'], sql: 'CREATE INDEX' },
      { phase: 2, kind: 'foreign_key', object: 'b', requires: ['b', 'a'], sql: 'ALTER TABLE' },
    ],
    text: '',
    problems: [],
    denied: null,
    confirm: [],
  }
}

describe('迁移方案', () => {
  it('两端都选好 schema 才能读取', () => {
    const p = emptyPlan(src)
    expect(ready(p)).toBe(false)
    expect(ready({ ...p, target: { profile: 'b', schema: 't' } })).toBe(true)
  })

  it('逐列覆盖：合并修改，恢复默认时去掉空的项', () => {
    let p = setColumn(emptyPlan(src), 't', 'c', { type: 'TEXT' })
    p = setColumn(p, 't', 'c', { name: 'C2' })
    expect(p.settings.tables).toEqual({ t: { columns: { c: { type: 'TEXT', name: 'C2' } } } })
    p = setColumn(p, 't', 'c', { type: undefined, name: undefined })
    expect(p.settings.tables).toEqual({})
    // 空串是有效的覆盖（不要默认值）
    p = setColumn(p, 't', 'c', { default: '' })
    expect(p.settings.tables.t.columns?.c).toEqual({ default: '' })
  })

  it('表、检查约束、序列的覆盖', () => {
    let p = setTable(emptyPlan(src), 't', { skip: true })
    expect(p.settings.tables.t).toEqual({ skip: true })
    p = setTable(p, 't', { skip: false })
    expect(p.settings.tables).toEqual({})
    p = setCheck(p, 't', 'ck', '')
    expect(p.settings.tables.t.checks).toEqual({ ck: '' })
    p = setCheck(p, 't', 'ck', undefined)
    expect(p.settings.tables).toEqual({})
    p = setSequence(p, 's1', { name: 'S2' })
    expect(p.settings.sequences).toEqual({ s1: { name: 'S2' } })
  })

  it('提示的汇总与同名表', () => {
    const pv = preview()
    expect(notesOf(pv, 'table', 'a').length).toBe(3)
    expect(notesOf(pv, 'table', 'a', 'x').map((n) => n.message)).toEqual(['m1'])
    expect(notesOf(pv, 'table', 'a', null).map((n) => n.message)).toEqual(['m3'])
    expect(worst(notesOf(pv, 'table', 'a'))).toBe('unsupported')
    expect(worst([])).toBeNull()
    let p = emptyPlan(src)
    expect(unchosen(p, pv)).toEqual(['a'])
    p = setExisting(p, ['a'], 'missing_only')
    expect(unchosen(p, pv)).toEqual([])
    expect(summary(pv)).toEqual({ tables: 1, sequences: 0, constraints: 2 })
  })

  it('打开另存的方案：补默认值，格式不对返回 null', () => {
    const p = parsePlan(JSON.stringify({ source: src, target: { profile: 'b', schema: 't' }, settings: { type_rules: [{ source: 'CLOB', target: 'TEXT' }] } }))
    expect(p?.settings.options).toEqual({ name_case: null, comments: true, checks: true })
    expect(p?.settings.type_rules).toEqual([{ source: 'CLOB', target: 'TEXT' }])
    expect(p?.tables).toBeNull()
    expect(parsePlan('{"source": 1}')).toBeNull()
    expect(parsePlan('not json')).toBeNull()
  })
})
