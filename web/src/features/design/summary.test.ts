import { describe, expect, it } from 'vitest'

import type { DesignColumn } from '../../shared/api'
import i18n from '../../shared/i18n'
import { describeChange, scriptOf } from './summary'

const col = (name: string, native: string, extra: Partial<DesignColumn> = {}): DesignColumn => ({
  name,
  ordinal: 1,
  data_type: { native, logical: { kind: 'other' } },
  nullable: true,
  default: null,
  comment: null,
  auto_increment: false,
  generated: null,
  ...extra,
})

describe('变更摘要', () => {
  it('修改列：每个改了的方面一行', async () => {
    await i18n.changeLanguage('zh-CN')
    const lines = describeChange({
      kind: 'alter_column',
      from: col('price', 'NUMBER(10,2)'),
      to: col('price', 'NUMBER(12,2)', { nullable: false, default: '0', comment: '价格' }),
    })
    expect(lines).toEqual([
      '修改列 price：类型 NUMBER(10,2) → NUMBER(12,2)',
      '修改列 price：不允许为空',
      '修改列 price：默认值改为 0',
      '修改列 price 的注释',
    ])
    expect(describeChange({ kind: 'rename_column', from: 'a', to: 'b', column: col('b', 'int') })).toEqual(['列 a 改名为 b'])
    expect(describeChange({ kind: 'add_unique', unique: { name: null, columns: ['a', 'b'] } })).toEqual(['新增唯一约束 a, b'])
  })

  it('英文区分单复数', async () => {
    await i18n.changeLanguage('en')
    const create = (n: number) =>
      describeChange({
        kind: 'create_table',
        table: {
          name: 't',
          comment: null,
          columns: Array.from({ length: n }, (_, i) => col(`c${i}`, 'int')),
          primary_key: null,
          uniques: [],
          foreign_keys: [],
          checks: [],
          indexes: [],
          triggers: [],
          partition: null,
        },
      })
    expect(create(1)).toEqual(['Create table t (1 column)'])
    expect(create(3)).toEqual(['Create table t (3 columns)'])
    await i18n.changeLanguage('zh-CN')
  })

  it('脚本：语句以分号结束，PL/SQL 块后加斜杠，核对行数写成注释', () => {
    const steps = [
      { kind: 'sql' as const, sql: 'CREATE TABLE t (a INT)', changes: [0] },
      { kind: 'count_check' as const, source: 'old', target: 'new', changes: [] },
      { kind: 'sql' as const, sql: 'CREATE TRIGGER x BEGIN NULL; END;', changes: [] },
    ]
    const text = scriptOf(steps, 'oracle')
    expect(text.split('\n\n')).toEqual([
      'CREATE TABLE t (a INT);',
      expect.stringMatching(/^-- 核对行数：old 与 new/),
      'CREATE TRIGGER x BEGIN NULL; END;\n/',
    ])
    expect(scriptOf(steps.slice(2), 'mysql')).toBe('CREATE TRIGGER x BEGIN NULL; END;')
    // SQL Server：每条之后一行 GO
    expect(scriptOf(steps.slice(2), 'sqlserver')).toBe('CREATE TRIGGER x BEGIN NULL; END;\nGO')
  })
})
