// 迁移方案的组成与修改（M5a 设计文档 3.3、5.2 节）：新方案、逐列覆盖（去掉空的项）、提示的汇总、
// 打开另存的方案时的校验、执行前的汇总。不依赖界面，单独测试。

import type {
  ColumnOverride,
  ExistingAction,
  MigrationEnd,
  MigrationNote,
  MigrationPlan,
  MigrationPreview,
  NoteLevel,
  TableOverride,
} from '../../shared/api'

export function emptyPlan(source: MigrationEnd): MigrationPlan {
  return {
    source,
    tables: null,
    sequences: null,
    target: { profile: '', schema: '' },
    settings: { options: { name_case: null, comments: true, checks: true }, type_rules: [], tables: {}, sequences: {} },
    existing: {},
  }
}

/** 两端都选好了 schema。 */
export function ready(plan: MigrationPlan): boolean {
  return !!(plan.source.profile && plan.source.schema && plan.target.profile && plan.target.schema)
}

/** 去掉值为 undefined 或 false 的项、空对象。 */
function compact<T extends object>(o: T): T | undefined {
  const entries = Object.entries(o).filter(([, v]) => v !== undefined && v !== false && !(typeof v === 'object' && v && !Object.keys(v).length))
  return entries.length ? (Object.fromEntries(entries) as T) : undefined
}

function withTable(plan: MigrationPlan, table: string, f: (t: TableOverride) => TableOverride): MigrationPlan {
  const tables = { ...plan.settings.tables }
  const next = compact(f({ ...tables[table] }))
  if (next) tables[table] = next
  else delete tables[table]
  return { ...plan, settings: { ...plan.settings, tables } }
}

/** 修改一列的覆盖：patch 中为 undefined 的项表示恢复默认。 */
export function setColumn(plan: MigrationPlan, table: string, column: string, patch: ColumnOverride): MigrationPlan {
  return withTable(plan, table, (t) => {
    const columns = { ...t.columns }
    const merged = compact({ ...columns[column], ...patch })
    if (merged) columns[column] = merged
    else delete columns[column]
    return { ...t, columns }
  })
}

/** 修改表的覆盖（目标名、不迁移）。 */
export function setTable(plan: MigrationPlan, table: string, patch: Pick<TableOverride, 'name' | 'skip'>): MigrationPlan {
  return withTable(plan, table, (t) => ({ ...t, ...patch }))
}

/** 修改检查约束的覆盖：expression 为 undefined 时恢复自动转换，空串表示不迁。 */
export function setCheck(plan: MigrationPlan, table: string, check: string, expression: string | undefined): MigrationPlan {
  return withTable(plan, table, (t) => {
    const checks = { ...t.checks }
    if (expression === undefined) delete checks[check]
    else checks[check] = expression
    return { ...t, checks }
  })
}

export function setSequence(plan: MigrationPlan, name: string, patch: { name?: string; skip?: boolean }): MigrationPlan {
  const sequences = { ...plan.settings.sequences }
  const next = compact({ ...sequences[name], ...patch })
  if (next) sequences[name] = next
  else delete sequences[name]
  return { ...plan, settings: { ...plan.settings, sequences } }
}

/** 给若干同名表设置处理方式。 */
export function setExisting(plan: MigrationPlan, tables: string[], action: ExistingAction): MigrationPlan {
  const existing = { ...plan.existing }
  for (const t of tables) existing[t] = action
  return { ...plan, existing }
}

const RANK: Record<NoteLevel, number> = { info: 0, lossy: 1, unsupported: 2 }

/** 一组提示中最严重的级别；没有提示时为 null。 */
export function worst(notes: MigrationNote[]): NoteLevel | null {
  return notes.reduce<NoteLevel | null>((w, n) => (w === null || RANK[n.level] > RANK[w] ? n.level : w), null)
}

/** 某个对象（某一列）的提示。`column` 为 null 时取对象本身（不属于某一列）的提示。 */
export function notesOf(p: MigrationPreview, kind: 'table' | 'sequence', object: string, column?: string | null): MigrationNote[] {
  return p.draft.notes.filter(
    (n) => n.kind === kind && n.object === object && (column === undefined || (column === null ? !n.column : n.column === column)),
  )
}

/** 目标中已有、还没选处理方式的表。 */
export function unchosen(plan: MigrationPlan, p: MigrationPreview): string[] {
  return p.draft.tables.filter((t) => t.exists && !plan.existing[t.source]).map((t) => t.source)
}

/** 执行前的汇总：建几张表、几个序列、几条索引和约束。 */
export function summary(p: MigrationPreview): { tables: number; sequences: number; constraints: number } {
  const count = (kinds: string[]) => p.statements.filter((s) => kinds.includes(s.kind)).length
  return {
    tables: count(['create_table']),
    sequences: count(['sequence']),
    constraints: count(['index', 'unique', 'check', 'foreign_key']),
  }
}

/** 一个对象的语句（DDL 预览）。 */
export function statementsOf(p: MigrationPreview, object: string): string {
  return p.statements
    .filter((s) => s.object === object)
    .map((s) => `${s.sql};`)
    .join('\n\n')
}

const isEnd = (v: unknown): v is MigrationEnd =>
  typeof (v as MigrationEnd)?.profile === 'string' && typeof (v as MigrationEnd).schema === 'string'
const isList = (v: unknown) => v === null || (Array.isArray(v) && v.every((x) => typeof x === 'string'))

/** 解析另存的方案；格式不对时返回 null。缺的设置项补默认值。 */
export function parsePlan(text: string): MigrationPlan | null {
  try {
    const v = JSON.parse(text) as Partial<MigrationPlan>
    if (!isEnd(v.source) || !isEnd(v.target) || !isList(v.tables ?? null) || !isList(v.sequences ?? null)) return null
    const base = emptyPlan(v.source)
    const s = (v.settings ?? {}) as Partial<MigrationPlan['settings']>
    return {
      ...base,
      tables: v.tables ?? null,
      sequences: v.sequences ?? null,
      target: v.target,
      settings: {
        options: { ...base.settings.options, ...s.options },
        type_rules: Array.isArray(s.type_rules) ? s.type_rules : [],
        tables: s.tables ?? {},
        sequences: s.sequences ?? {},
      },
      existing: v.existing ?? {},
    }
  } catch {
    return null
  }
}
