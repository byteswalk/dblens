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

  it('变体未知时按驱动选解析器，按兼容档处理', () => {
    expect(sqlDialect('oracle', null)).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
    expect(sqlDialect('mysql')).toEqual({ dialect: 'mysql', strict: false, grammar: 'MySQL' })
  })
})
