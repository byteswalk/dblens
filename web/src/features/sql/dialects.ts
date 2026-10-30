// 驱动 / 变体 → 解析器与档位（M1b2 设计文档 3.2 节）。
// 标准库的语法错误标红；兼容库（在 MySQL / PG / Oracle 语法上有扩展）只标黄"可能有误"，避免误报显得刺眼。
// 变体未知（还没连接过）时按驱动取解析器，按兼容档处理。
// 人大金仓、openGauss 的 Oracle 兼容模式按 PL/SQL 的语法（兼容档）、关键字和语句拆分处理。

import type { Compat, DriverId } from '../../shared/api'
import type { Dialect } from './types'

export interface SqlDialect {
  dialect: Dialect
  /** 标准档：语法错误按错误（红色）标出。 */
  strict: boolean
  /** 检查所用语法的名称，兼容档的提示中说明。 */
  grammar: 'MySQL' | 'PostgreSQL' | 'Oracle' | 'T-SQL'
}

const STRICT = new Set(['mysql', 'mariadb', 'postgres', 'oracle', 'sqlserver'])

export function sqlDialect(driver: DriverId, variant?: string | null, compat?: Compat | null): SqlDialect {
  const strict = !!variant && STRICT.has(variant)
  if (driver === 'postgres' && compat === 'oracle') return { dialect: 'plsql', strict: false, grammar: 'Oracle' }
  switch (driver) {
    case 'mysql':
      return { dialect: 'mysql', strict, grammar: 'MySQL' }
    case 'postgres':
      return { dialect: 'postgres', strict, grammar: 'PostgreSQL' }
    case 'oracle':
    case 'dameng':
      return { dialect: 'plsql', strict, grammar: 'Oracle' }
    case 'sqlserver':
      return { dialect: 'tsql', strict, grammar: 'T-SQL' }
  }
}
