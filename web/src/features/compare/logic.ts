// 比对标签的纯逻辑（M4a 设计文档第 7 节）：默认勾选、依赖提示、变更的摘要、两边定义的文本对照、采用推测的改名。

import type {
  CompareRenames,
  CompareResult,
  DesignTableModel,
  RenameSuggestion,
  SchemaChange,
  TableDiff,
  TableStatus,
} from '../../shared/api'
import { t } from '../../shared/i18n'
import { describeChange } from '../design/summary'

/** 删除表、删除列：会丢数据，默认不勾选，勾选后标红。 */
export function isDangerous(c: SchemaChange): boolean {
  return c.kind === 'drop_table' || (c.kind === 'table' && c.change.kind === 'drop_column')
}

/** 默认勾选：除删除表、删除列以外的全部变更。 */
export function defaultSelection(tables: TableDiff[]): number[] {
  return tables.flatMap((d) => d.items.filter((i) => !isDangerous(i.change)).map((i) => i.id))
}

export function describeItem(c: SchemaChange): string {
  if (c.kind === 'drop_table') return t('model.compare.change.dropTable', { name: c.name })
  if (c.kind === 'rename_table') return t('model.compare.change.renameTable', { from: c.from, to: c.to })
  return describeChange(c.change).join('；')
}

/** 表的显示名：两边名字不同时写成"目标 → 源"。 */
export function tableLabel(d: TableDiff): string {
  if (d.source && d.target && d.source !== d.target) return `${d.target} → ${d.source}`
  return d.source ?? d.target ?? ''
}

export const STATUSES: TableStatus[] = ['added', 'removed', 'renamed', 'changed', 'same']

/** 勾选的组合中缺了依赖的变更：加外键引用的新表没有勾选建表；删除的表还被其他表的外键引用着。 */
export function dependencyIssues(r: CompareResult, selected: ReadonlySet<number>): string[] {
  const out = new Set<string>()
  const tables = r.diff.tables
  const created = (name: string) => {
    const d = tables.find((x) => x.status === 'added' && x.source === name)
    return !d || d.items.every((i) => selected.has(i.id))
  }
  for (const d of tables) {
    for (const i of d.items) {
      if (!selected.has(i.id) || i.change.kind !== 'table') continue
      const c = i.change.change
      const refs =
        c.kind === 'add_foreign_key'
          ? [c.key]
          : c.kind === 'create_table'
            ? c.table.foreign_keys
            : []
      for (const k of refs) {
        if (!k.ref_schema && k.ref_table !== d.source && !created(k.ref_table)) {
          out.add(t('model.compare.dep.create', { table: d.source ?? '', ref: k.ref_table }))
        }
      }
    }
  }
  // 要删除的表（目标中的名字）
  const dropped = new Set(
    tables.flatMap((d) => d.items.filter((i) => selected.has(i.id) && i.change.kind === 'drop_table').map(() => d.target ?? '')),
  )
  for (const x of dropped) {
    for (const tt of r.target_tables) {
      if (dropped.has(tt.name)) continue
      const d = tables.find((y) => y.target === tt.name)
      for (const k of tt.foreign_keys) {
        if (k.ref_schema || k.ref_table !== x) continue
        const droppedFk = d?.items.some(
          (i) =>
            selected.has(i.id) &&
            i.change.kind === 'table' &&
            i.change.change.kind === 'drop_foreign_key' &&
            i.change.change.key.name === k.name,
        )
        if (!droppedFk) out.add(t('model.compare.dep.drop', { table: x, by: tt.name }))
      }
    }
  }
  return [...out]
}

const cols = (c: string[]) => `(${c.join(', ')})`

/** 表定义的文本（右侧两边对照）：每列、每个约束、每个索引一行。 */
export function tableText(tb: DesignTableModel | undefined): string[] {
  if (!tb) return []
  const lines: string[] = []
  if (tb.comment) lines.push(`-- ${tb.comment}`)
  for (const c of tb.columns) {
    let s = `${c.name} ${c.data_type.native}`
    if (!c.nullable) s += ' NOT NULL'
    if (c.default) s += ` DEFAULT ${c.default}`
    if (c.auto_increment) s += ' AUTO'
    if (c.generated) s += ` AS (${c.generated})`
    if (c.comment) s += ` -- ${c.comment}`
    lines.push(s)
  }
  const pk = tb.primary_key
  if (pk) lines.push(`PRIMARY KEY ${pk.name ?? ''} ${cols(pk.columns)}`.replace('  ', ' '))
  for (const u of tb.uniques) lines.push(`UNIQUE ${u.name ?? ''} ${cols(u.columns)}`.replace('  ', ' '))
  for (const k of tb.foreign_keys) {
    const ref = `${k.ref_schema ? `${k.ref_schema}.` : ''}${k.ref_table}${cols(k.ref_columns)}`
    let s = `FOREIGN KEY ${k.name ?? ''} ${cols(k.columns)} → ${ref}`.replace('  ', ' ')
    if (k.on_delete) s += ` ON DELETE ${k.on_delete}`
    if (k.on_update) s += ` ON UPDATE ${k.on_update}`
    lines.push(s)
  }
  for (const c of tb.checks) lines.push(`CHECK ${c.name ?? ''} (${c.expression})`.replace('  ', ' '))
  for (const x of tb.indexes) {
    const c = x.columns.map((i) => (i.descending ? `${i.name} DESC` : i.name))
    lines.push(`${x.unique ? 'UNIQUE INDEX' : 'INDEX'} ${x.name} ${cols(c)}${x.method ? ` USING ${x.method}` : ''}`)
  }
  return lines
}

/** 两边的行：另一边没有的行标为不同。 */
export function compareLines(a: string[], b: string[]): { text: string; diff: boolean }[][] {
  const sa = new Set(a)
  const sb = new Set(b)
  return [a.map((text) => ({ text, diff: !sb.has(text) })), b.map((text) => ({ text, diff: !sa.has(text) }))]
}

/** 采用一条推测的改名。 */
export function acceptSuggestion(r: CompareRenames, s: RenameSuggestion): CompareRenames {
  if (s.table === null) return { ...r, tables: { ...r.tables, [s.source]: s.target } }
  return { ...r, columns: { ...r.columns, [s.table]: { ...(r.columns[s.table] ?? {}), [s.source]: s.target } } }
}

export const EMPTY_RENAMES: CompareRenames = { tables: {}, columns: {} }
