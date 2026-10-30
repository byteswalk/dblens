// 变更列表的可读摘要（保存确认框中列出），以及复制、在查询标签中打开用的脚本文本。

import type { DesignColumn, DesignStep, DriverId, TableChange } from '../../shared/api'
import { t } from '../../shared/i18n'

const list = (cols: string[]) => cols.join(', ')
const text = (v: string | null) => v?.trim() || null

/** 一项变更的摘要：修改列时每个改了的方面一行。 */
export function describeChange(c: TableChange): string[] {
  switch (c.kind) {
    case 'create_table':
      return [t('design.change.createTable', { name: c.table.name, count: c.table.columns.length })]
    case 'set_table_comment':
      return [t('design.change.tableComment')]
    case 'add_column':
      return [t('design.change.addColumn', { name: c.column.name, type: c.column.data_type.native })]
    case 'drop_column':
      return [t('design.change.dropColumn', { name: c.name })]
    case 'rename_column':
      return [t('design.change.renameColumn', { from: c.from, to: c.to })]
    case 'alter_column':
      return alterLines(c.from, c.to)
    case 'move_column':
      return [t('design.change.moveColumn', { name: c.column.name })]
    case 'add_primary_key':
      return [t('design.change.addPrimaryKey', { columns: list(c.key.columns) })]
    case 'drop_primary_key':
      return [t('design.change.dropPrimaryKey')]
    case 'add_unique':
      return [t('design.change.addUnique', { name: c.unique.name ?? list(c.unique.columns) })]
    case 'drop_unique':
      return [t('design.change.dropUnique', { name: c.unique.name ?? list(c.unique.columns) })]
    case 'add_check':
      return [t('design.change.addCheck', { name: c.check.name ?? c.check.expression })]
    case 'drop_check':
      return [t('design.change.dropCheck', { name: c.check.name ?? c.check.expression })]
    case 'add_foreign_key':
      return [t('design.change.addForeignKey', { name: c.key.name ?? list(c.key.columns), table: c.key.ref_table })]
    case 'drop_foreign_key':
      return [t('design.change.dropForeignKey', { name: c.key.name ?? list(c.key.columns) })]
    case 'add_index':
      return [t('design.change.addIndex', { name: c.index.name })]
    case 'drop_index':
      return [t('design.change.dropIndex', { name: c.index.name })]
    case 'rename_index':
      return [t('design.change.renameIndex', { from: c.from, to: c.index.name })]
  }
}

function alterLines(from: DesignColumn, to: DesignColumn): string[] {
  const name = to.name
  const out: string[] = []
  if (from.data_type.native.trim().toLowerCase() !== to.data_type.native.trim().toLowerCase()) {
    out.push(t('design.change.columnType', { name, from: from.data_type.native, to: to.data_type.native }))
  }
  if (from.nullable !== to.nullable) {
    out.push(t(to.nullable ? 'design.change.columnNull' : 'design.change.columnNotNull', { name }))
  }
  if (text(from.default) !== text(to.default)) {
    out.push(t('design.change.columnDefault', { name, value: text(to.default) ?? t('design.change.none') }))
  }
  if (text(from.comment) !== text(to.comment)) out.push(t('design.change.columnComment', { name }))
  if (from.auto_increment !== to.auto_increment) {
    out.push(t(to.auto_increment ? 'design.change.columnAutoOn' : 'design.change.columnAutoOff', { name }))
  }
  if (text(from.generated) !== text(to.generated)) out.push(t('design.change.columnGenerated', { name }))
  return out
}

/**
 * 预览中的语句拼成脚本：每条以 `;` 结束（Oracle、达梦的 PL/SQL 块之后加一行 `/`；SQL Server 每条之后加一行 `GO`），
 * 由程序执行的核对行数写成注释。
 */
export function scriptOf(steps: DesignStep[], driver: DriverId): string {
  const plsql = driver === 'oracle' || driver === 'dameng'
  return steps
    .map((s) => {
      if (s.kind === 'count_check') return `-- ${t('design.save.countCheck', { source: s.source, target: s.target })}`
      const sql = s.sql.trimEnd()
      if (driver === 'sqlserver') return `${sql}\nGO`
      if (sql.endsWith(';')) return plsql ? `${sql}\n/` : sql
      return `${sql};`
    })
    .join('\n\n')
}
