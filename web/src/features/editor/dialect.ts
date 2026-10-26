// 编辑器与方言的对应（M1 设计文档 2.1 节）：Monaco 高亮用的语言、sql-formatter 的方言。
// 会话打开前只知道驱动，打开后用识别出的变体（如 MariaDB、TiDB）细化格式化方言。

import { type FormatOptionsWithLanguage, format } from 'sql-formatter'

import type { DriverId } from '../../shared/api'

/** Monaco 的语言：MySQL 系、PG 系用 Monaco 自带的规则，Oracle 和达梦用 plsql（monaco.ts 注册）。 */
export function editorLanguage(driver: DriverId): 'mysql' | 'pgsql' | 'plsql' {
  switch (driver) {
    case 'mysql':
      return 'mysql'
    case 'postgres':
      return 'pgsql'
    case 'oracle':
    case 'dameng':
      return 'plsql'
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

/** 状态栏显示的方言名称。 */
export const VARIANT_NAMES: Record<string, string> = {
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
  dameng: '达梦',
}
