// 表设计器（M3 设计文档第 7 节）的纯逻辑：草稿与改名跟踪、类型的拆分与拼接、改动标记、整理成请求。
// 草稿中的每一行有不随改名变化的 key 和原表中的名字 origin（新增的为 null）；原有的行删除时只标记 deleted，可以还原。

import type {
  DesignCheck,
  DesignColumn,
  DesignForeignKey,
  DesignIndex,
  DesignKey,
  DesignRenames,
  DesignTableModel,
  TypeEntry,
  TypeParams,
} from '../../shared/api'

interface Row {
  key: string
  origin: string | null
  deleted?: boolean
}

export type DraftColumn = DesignColumn & Row
export type DraftKey = DesignKey & Row
export type DraftCheck = DesignCheck & Row
export type DraftForeignKey = DesignForeignKey & Row
export type DraftIndex = DesignIndex & Row

export interface Draft {
  name: string
  comment: string | null
  columns: DraftColumn[]
  primaryKey: DesignKey | null
  uniques: DraftKey[]
  checks: DraftCheck[]
  foreignKeys: DraftForeignKey[]
  indexes: DraftIndex[]
  partition: string | null
}

let seq = 0
export const newKey = () => `r${++seq}`

/** 打开已有表：原表的每一行记下原名。没有名字的约束用序号当原名（只用于对照改动标记）。 */
export function fromTable(t: DesignTableModel): Draft {
  const row = <T extends object>(x: T, origin: string | null): T & Row => ({ ...x, key: newKey(), origin })
  return {
    name: t.name,
    comment: t.comment,
    columns: t.columns.map((c) => row(c, c.name)),
    primaryKey: t.primary_key && { ...t.primary_key, columns: [...t.primary_key.columns] },
    uniques: t.uniques.map((u, i) => row(u, u.name ?? `#${i}`)),
    checks: t.checks.map((c, i) => row(c, c.name ?? `#${i}`)),
    foreignKeys: t.foreign_keys.map((f, i) => row(f, f.name ?? `#${i}`)),
    indexes: t.indexes.map((x) => row(x, x.name)),
    partition: t.partition,
  }
}

export function emptyDraft(): Draft {
  return {
    name: '',
    comment: null,
    columns: [],
    primaryKey: null,
    uniques: [],
    checks: [],
    foreignKeys: [],
    indexes: [],
    partition: null,
  }
}

export function emptyColumn(name = ''): DraftColumn {
  return {
    key: newKey(),
    origin: null,
    name,
    ordinal: 0,
    data_type: { native: '', logical: { kind: 'other' } },
    nullable: true,
    default: null,
    comment: null,
    auto_increment: false,
    generated: null,
  }
}

const live = <T extends Row>(rows: T[]) => rows.filter((r) => !r.deleted)

function strip<T extends Row>(r: T): Omit<T, 'key' | 'origin' | 'deleted'> {
  const { key: _k, origin: _o, deleted: _d, ...rest } = r
  return rest
}

/** 整理成请求的表结构：去掉标为删除的行；删除的列也从主键、唯一约束、索引中去掉（去掉后为空的项一并去掉），引用它的外键整个去掉。 */
export function toModel(d: Draft): DesignTableModel {
  const gone = new Set(d.columns.filter((c) => c.deleted && c.origin !== null).map((c) => c.name))
  const keep = (cols: string[]) => cols.filter((c) => !gone.has(c))
  const pk = d.primaryKey && keep(d.primaryKey.columns)
  return {
    name: d.name.trim(),
    comment: d.comment?.trim() ? d.comment : null,
    columns: live(d.columns).map((c, i) => ({ ...strip(c), ordinal: i + 1 })),
    primary_key: pk?.length ? { name: d.primaryKey!.name, columns: pk } : null,
    uniques: live(d.uniques)
      .map((u) => ({ ...strip(u), columns: keep(u.columns) }))
      .filter((u) => u.columns.length),
    checks: live(d.checks).map(strip),
    foreign_keys: live(d.foreignKeys)
      .filter((f) => !f.columns.some((c) => gone.has(c)))
      .map(strip),
    indexes: live(d.indexes)
      .map((x) => ({ ...strip(x), columns: x.columns.filter((c) => !gone.has(c.name)) }))
      .filter((x) => x.columns.length),
    triggers: [],
    partition: d.partition,
  }
}

/** 改了名的列和索引：新名 → 原名。 */
export function renamesOf(d: Draft): DesignRenames {
  const map = (rows: (Row & { name: string })[]) =>
    Object.fromEntries(
      rows.filter((r) => !r.deleted && r.origin !== null && r.origin !== r.name).map((r) => [r.name, r.origin as string]),
    )
  return { columns: map(d.columns), indexes: map(d.indexes) }
}

/** 有没有未保存的改动：新建表时有名字或有列就算有。 */
export function isDirty(d: Draft, original: Draft | null): boolean {
  if (!original) return d.name.trim() !== '' || d.columns.length > 0
  return JSON.stringify(toModel(d)) !== JSON.stringify(toModel(original))
}

// —— 列的编辑 ——

const renameIn = (cols: string[], from: string, to: string) => cols.map((c) => (c === from ? to : c))

/** 修改一列；改名时同时改主键、唯一约束、索引、外键中引用它的地方（指向本表的外键的引用列也改）。 */
export function updateColumn(d: Draft, key: string, patch: Partial<DesignColumn>): Draft {
  const col = d.columns.find((c) => c.key === key)
  if (!col) return d
  const next = { ...d, columns: d.columns.map((c) => (c.key === key ? { ...c, ...patch } : c)) }
  const from = col.name
  const to = patch.name
  if (to === undefined || to === from || from === '') return next
  return {
    ...next,
    primaryKey: next.primaryKey && { ...next.primaryKey, columns: renameIn(next.primaryKey.columns, from, to) },
    uniques: next.uniques.map((u) => ({ ...u, columns: renameIn(u.columns, from, to) })),
    indexes: next.indexes.map((x) => ({
      ...x,
      columns: x.columns.map((c) => (c.name === from ? { ...c, name: to } : c)),
    })),
    foreignKeys: next.foreignKeys.map((f) => ({
      ...f,
      columns: renameIn(f.columns, from, to),
      ref_columns: f.ref_table === d.name ? renameIn(f.ref_columns, from, to) : f.ref_columns,
    })),
  }
}

/** 在 `after` 那一行之后插入新列（不给时加在最后）。 */
export function addColumn(d: Draft, column: DraftColumn, after?: string): Draft {
  const columns = [...d.columns]
  const i = after ? columns.findIndex((c) => c.key === after) : -1
  columns.splice(i < 0 ? columns.length : i + 1, 0, column)
  return { ...d, columns }
}

/** 删除一列：新增的直接去掉，原有的标记删除（可还原）。 */
export function removeColumn(d: Draft, key: string): Draft {
  const col = d.columns.find((c) => c.key === key)
  if (!col) return d
  if (col.origin === null) return { ...d, columns: d.columns.filter((c) => c.key !== key) }
  return { ...d, columns: d.columns.map((c) => (c.key === key ? { ...c, deleted: true } : c)) }
}

/** 还原一列：恢复原表中的定义（改名同样还原，引用它的地方跟着改回）。 */
export function restoreColumn(d: Draft, key: string, original: Draft): Draft {
  const col = d.columns.find((c) => c.key === key)
  const orig = original.columns.find((c) => c.origin === col?.origin)
  if (!col || !orig) return d
  const renamed = updateColumn(d, key, { name: orig.name })
  return { ...renamed, columns: renamed.columns.map((c) => (c.key === key ? { ...orig, key } : c)) }
}

/** 上移（-1）或下移（1）一列。 */
export function moveColumn(d: Draft, key: string, delta: -1 | 1): Draft {
  const i = d.columns.findIndex((c) => c.key === key)
  const j = i + delta
  if (i < 0 || j < 0 || j >= d.columns.length) return d
  const columns = [...d.columns]
  ;[columns[i], columns[j]] = [columns[j], columns[i]]
  return { ...d, columns }
}

/** 勾选或取消主键列：新加的列排在主键的最后。 */
export function togglePrimaryKey(d: Draft, column: string): Draft {
  const cols = d.primaryKey?.columns ?? []
  const next = cols.includes(column) ? cols.filter((c) => c !== column) : [...cols, column]
  const primaryKey = next.length ? { name: d.primaryKey?.name ?? null, columns: next } : null
  // 主键列不能为空
  const columns = d.columns.map((c) => (c.name === column && next.includes(column) ? { ...c, nullable: false } : c))
  return { ...d, primaryKey, columns }
}

/** 在列表中挪动一项（主键列的顺序等）。 */
export function moveItem<T>(list: T[], i: number, delta: -1 | 1): T[] {
  const j = i + delta
  if (j < 0 || j >= list.length) return list
  const out = [...list]
  ;[out[i], out[j]] = [out[j], out[i]]
  return out
}

// —— 类型 ——

export interface TypeParts {
  name: string
  length: string
  scale: string
  unit: '' | 'BYTE' | 'CHAR'
}

/** 拆开原生类型：`NUMBER(10,2)` → NUMBER、10、2；`VARCHAR2(20 CHAR)` → 单位 CHAR；
 * `timestamp(6) with time zone` → 名称 `timestamp with time zone`、长度 6。参数中有引号的（如 enum）整体当作名称。 */
export function splitType(native: string): TypeParts {
  const empty = { name: native.trim(), length: '', scale: '', unit: '' as const }
  const m = /^([^()]*)(?:\(([^)]*)\))?(.*)$/.exec(native)
  if (!m || native.includes("'")) return empty
  const [base, args, suffix] = [m[1].trim(), m[2] ?? '', m[3].trim()]
  const [first = '', second = ''] = args.split(',').map((a) => a.trim())
  const unit = /\s+(BYTE|CHAR)$/i.exec(first)
  return {
    name: suffix ? `${base} ${suffix}` : base,
    length: unit ? first.slice(0, unit.index).trim() : first,
    scale: second,
    unit: unit ? (unit[1].toUpperCase() as 'BYTE' | 'CHAR') : '',
  }
}

/** 拼回原生类型。名称中的 WITH / WITHOUT / UNSIGNED / ZEROFILL 之前插入参数（`TIMESTAMP(6) WITH TIME ZONE`）。 */
export function joinType(p: TypeParts): string {
  const name = p.name.trim()
  const length = p.length.trim()
  const scale = p.scale.trim()
  if (!length) return name
  const args = `${length}${p.unit ? ` ${p.unit}` : ''}${scale ? `,${scale}` : ''}`
  const m = /\s+(with|without|unsigned|zerofill)\b/i.exec(name)
  return m ? `${name.slice(0, m.index)}(${args})${name.slice(m.index)}` : `${name}(${args})`
}

export function findType(name: string, types: TypeEntry[]): TypeEntry | undefined {
  const n = name.trim().toLowerCase()
  return types.find((t) => t.name.toLowerCase() === n)
}

/** 类型的参数形式：目录里没有的类型两项都可以填（带括号的整体名称如 enum 不能再填）。 */
export function typeParams(name: string, types: TypeEntry[]): TypeParams {
  return findType(name, types)?.params ?? (name.includes('(') ? 'none' : 'precision_scale')
}

// —— 改动标记 ——

export type RowState = 'new' | 'changed' | 'deleted' | null

const COLUMN_FIELDS = ['name', 'type', 'nullable', 'default', 'comment', 'auto_increment', 'generated'] as const
export type ColumnField = (typeof COLUMN_FIELDS)[number]

function columnValue(c: DesignColumn, f: ColumnField): unknown {
  if (f === 'type') return c.data_type.native.trim().toLowerCase()
  const v = c[f]
  return typeof v === 'string' ? v.trim() || null : v
}

/** 原有的列改了哪些字段。 */
export function changedFields(c: DraftColumn, original: Draft | null): Set<ColumnField> {
  const orig = original?.columns.find((o) => o.origin === c.origin)
  if (!orig || c.origin === null) return new Set()
  return new Set(COLUMN_FIELDS.filter((f) => columnValue(c, f) !== columnValue(orig, f)))
}

/** 行的状态：新增、修改过、标为删除。`same` 比较原有行和原表中的那一行。 */
export function rowState<T extends Row>(row: T, originals: T[] | undefined, same: (a: T, b: T) => boolean): RowState {
  if (row.deleted) return 'deleted'
  if (row.origin === null) return 'new'
  const orig = originals?.find((o) => o.origin === row.origin)
  return !orig || !same(row, orig) ? 'changed' : null
}

export function columnState(c: DraftColumn, original: Draft | null): RowState {
  if (c.deleted) return 'deleted'
  if (c.origin === null) return 'new'
  return changedFields(c, original).size ? 'changed' : null
}

/** 去掉行标识后比较内容。 */
export const sameContent = <T extends Row>(a: T, b: T) => JSON.stringify(strip(a)) === JSON.stringify(strip(b))
