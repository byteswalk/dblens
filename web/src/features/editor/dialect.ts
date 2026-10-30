// 编辑器与方言的对应（M1 设计文档 2.1 节）：Monaco 高亮用的语言、sql-formatter 的方言。
// 会话打开前只知道驱动，打开后用识别出的变体（如 MariaDB、TiDB）细化格式化方言。

import { type FormatOptionsWithLanguage, format } from 'sql-formatter'

import type { Compat, DriverId } from '../../shared/api'
import { t } from '../../shared/i18n'

/**
 * Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle、达梦和 PG 系的 Oracle 兼容模式用 plsql（monaco.ts 注册），
 * SQL Server 用 Monaco 自带的 sql（按 T-SQL 的关键字）。
 */
export function editorLanguage(driver: DriverId, compat?: Compat | null): 'mysql' | 'pgsql' | 'plsql' | 'sql' {
  if (driver === 'postgres' && compat === 'oracle') return 'plsql'
  switch (driver) {
    case 'mysql':
      return 'mysql'
    case 'postgres':
      return 'pgsql'
    case 'oracle':
    case 'dameng':
      return 'plsql'
    case 'sqlserver':
      return 'sql'
  }
}

type FormatterLanguage = FormatOptionsWithLanguage['language']

/** sql-formatter 的方言：变体已知时按变体，否则按驱动。 */
export function formatterLanguage(driver: DriverId, variant?: string | null): FormatterLanguage {
  switch (variant) {
    case 'mariadb':
      return 'mariadb'
    case 'tidb':
      return 'tidb'
  }
  switch (driver) {
    case 'mysql':
      return 'mysql'
    case 'postgres':
      return 'postgresql'
    case 'oracle':
    case 'dameng':
      return 'plsql'
    case 'sqlserver':
      return 'transactsql'
  }
}

/** 格式化 SQL。关键字大小写保持原样；解析不了时抛出 sql-formatter 的错误。 */
export function formatSql(sql: string, driver: DriverId, variant?: string | null): string {
  return format(sql, {
    language: formatterLanguage(driver, variant),
    tabWidth: 2,
    keywordCase: 'preserve',
    linesBetweenQueries: 1,
  })
}

const VARIANT_NAMES: Record<string, string> = {
  mysql: 'MySQL',
  mariadb: 'MariaDB',
  tidb: 'TiDB',
  oceanbase: 'OceanBase',
  polardb: 'PolarDB',
  postgres: 'PostgreSQL',
  kingbase: 'KingbaseES',
  opengauss: 'openGauss',
  gaussdb: 'GaussDB',
  highgo: 'HighGo',
  oracle: 'Oracle',
  sqlserver: 'SQL Server',
}

/** 变体的简短名称（状态栏、对象树用）；达梦的名称随界面语言。不认识的变体返回 undefined。 */
export function variantName(variant: string | null | undefined): string | undefined {
  if (variant === 'dameng') return t('editor.dialect.dameng')
  return variant ? VARIANT_NAMES[variant] : undefined
}
