import { describe, expect, it } from 'vitest'

import { sqlDialect } from './dialects'

describe('sqlDialect', () => {
  it.each([
    ['mysql', 'mysql', 'mysql', true],
    ['mysql', 'mariadb', 'mysql', true],
    ['mysql', 'tidb', 'mysql', false],
    ['mysql', 'oceanbase', 'mysql', false],
    ['mysql', 'polardb', 'mysql', false],
    ['postgres', 'postgres', 'postgres', true],
    ['postgres', 'kingbase', 'postgres', false],
    ['postgres', 'opengauss', 'postgres', false],
    ['postgres', 'gaussdb', 'postgres', false],
    ['postgres', 'highgo', 'postgres', false],
    ['oracle', 'oracle', 'plsql', true],
    ['dameng', 'dameng', 'plsql', false],
  ] as const)('%s / %s → %s，标准档 %s', (driver, variant, dialect, strict) => {
    expect(sqlDialect(driver, variant)).toMatchObject({ dialect, strict })
  })

  it('人大金仓、openGauss 的 Oracle 兼容模式按 PL/SQL 处理（兼容档）', () => {
    expect(sqlDialect('postgres', 'kingbase', 'oracle')).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
    expect(sqlDialect('postgres', 'opengauss', 'native')).toMatchObject({ dialect: 'postgres' })
    expect(sqlDialect('mysql', 'tidb', 'oracle')).toMatchObject({ dialect: 'mysql' })
  })

  it('变体未知时按驱动选解析器，按兼容档处理', () => {
    expect(sqlDialect('oracle', null)).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
    expect(sqlDialect('mysql')).toEqual({ dialect: 'mysql', strict: false, grammar: 'MySQL' })
  })
})
