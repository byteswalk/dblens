// 补全项（M1b2 设计文档第 4 节）：worker 给出光标处该出现什么（Suggest），这里查元数据生成补全项。
// 不依赖 Monaco，provider.ts 再转成 Monaco 的格式。

import { builtinFunctions } from './functions'
import { type MetaReader, findName } from './metadata'
import type { Dialect, Suggest, TableRef } from './types'

export type ItemKind = 'keyword' | 'schema' | 'table' | 'view' | 'synonym' | 'sequence' | 'column' | 'function'

export interface Item {
  label: string
  kind: ItemKind
  /** 补全列表右侧的说明：列的类型和所属表、对象的注释等。 */
  detail?: string
  insertText: string
  /** 排序：列在前（按表中的顺序），然后是表、schema、函数、关键字。 */
  sortText: string
}

const ORDER: Record<ItemKind, string> = {
  column: '0',
  table: '1',
  view: '1',
  synonym: '1',
  sequence: '2',
  schema: '2',
  function: '3',
  keyword: '4',
}

/** 名称需要时加引号：含特殊字符、是 PL/SQL 中的非大写名称或 PG 中的非小写名称。 */
export function quoteIdent(name: string, d: Dialect): string {
  const plain = /^[A-Za-z_][A-Za-z0-9_$#]*$/.test(name)
  switch (d) {
    case 'mysql':
      return plain ? name : `\`${name.replaceAll('`', '``')}\``
    case 'postgres':
      return plain && name === name.toLowerCase() ? name : `"${name.replaceAll('"', '""')}"`
    case 'plsql':
      return plain && name === name.toUpperCase() ? name : `"${name.replaceAll('"', '""')}"`
  }
}

export interface CompletionInput {
  suggest: Suggest
  dialect: Dialect
  /** 当前 schema（会话的，或连接配置推断的）；不知道时为 null。 */
  schema: string | null
  meta: MetaReader
}

/** order：同类中的次序（列按表中的顺序）；不给时按名称。 */
const item = (kind: ItemKind, label: string, d: Dialect, detail?: string | null, order?: number): Item => ({
  label,
  kind,
  detail: detail || undefined,
  insertText: kind === 'keyword' || kind === 'function' ? label : quoteIdent(label, d),
  sortText: ORDER[kind] + (order === undefined ? label : String(order).padStart(5, '0')),
})

export async function completionItems({ suggest, dialect, schema, meta }: CompletionInput): Promise<Item[]> {
  const want = new Set(suggest.want)
  const q = suggest.qualifier
  const out: Item[] = []
  let columnSeq = 0
  const schemas = await meta.schemas()
  const resolveSchema = (s: string | undefined) => (s === undefined ? schema : (findName(schemas, s) ?? s))

  /** 语句中的表 → 实际的 schema 和表名（大小写按元数据）。 */
  const resolveTable = async (ref: TableRef) => {
    const s = resolveSchema(ref.schema)
    if (!s) return null
    const obj = findName(await meta.objects(s), ref.name)
    return { schema: s, table: obj?.name ?? ref.name }
  }
  const columnsOf = async (ref: TableRef, withTable: boolean) => {
    const r = await resolveTable(ref)
    if (!r) return
    for (const c of await meta.columns(r.schema, r.table)) {
      const where = withTable ? ` · ${ref.alias ?? r.table}` : ''
      out.push(item('column', c.name, dialect, `${c.type}${where}${c.comment ? ` · ${c.comment}` : ''}`, columnSeq++))
    }
  }
  const objectsOf = async (s: string) => {
    for (const o of await meta.objects(s)) {
      const isTable = o.kind === 'table' || o.kind === 'synonym'
      if ((isTable && want.has('table')) || (o.kind === 'view' && (want.has('view') || want.has('table'))) || (o.kind === 'sequence' && want.has('column'))) {
        out.push(item(o.kind, o.name, dialect, o.comment))
      }
    }
  }
  const functionsOf = async (s: string | null) => {
    if (s) for (const f of await meta.functions(s)) out.push(item('function', f, dialect))
  }

  if (q.length === 0) {
    if (want.has('column')) for (const ref of suggest.tables) await columnsOf(ref, suggest.tables.length > 1)
    if ((want.has('table') || want.has('view')) && schema) await objectsOf(schema)
    if (want.has('table') || want.has('view') || want.has('schema')) for (const s of schemas) out.push(item('schema', s, dialect))
    if (want.has('function')) {
      for (const f of builtinFunctions(dialect)) out.push(item('function', f, dialect))
      await functionsOf(schema)
    }
    for (const k of suggest.keywords) out.push(item('keyword', k, dialect))
  } else if (q.length === 1) {
    const [name] = q
    // 别名或语句中的表名
    const ref =
      suggest.tables.find((t) => t.alias && t.alias.toUpperCase() === name.toUpperCase()) ??
      suggest.tables.find((t) => t.name.toUpperCase() === name.toUpperCase())
    const s = findName(schemas, name)
    if (ref) await columnsOf(ref, false)
    else if (s) {
      await objectsOf(s)
      if (want.has('function')) await functionsOf(s)
    } else if (want.has('column') && schema && findName(await meta.objects(schema), name)) {
      // 当前 schema 中的表，但语句里还没写（如 WHERE emp.）
      await columnsOf({ name }, false)
    }
  } else if (q.length === 2) {
    await columnsOf({ schema: q[0], name: q[1] }, false)
  }
  return dedupe(out)
}

/** 同名同类只留一个（如两张表的同名列合并说明）。 */
function dedupe(items: Item[]): Item[] {
  const seen = new Map<string, Item>()
  for (const i of items) {
    const key = `${i.kind}:${i.label}`
    const prev = seen.get(key)
    if (!prev) seen.set(key, i)
    else if (i.kind === 'column' && i.detail && prev.detail !== i.detail) prev.detail = `${prev.detail ?? ''}; ${i.detail}`
  }
  return [...seen.values()]
}

const COMMON_KEYWORDS = `SELECT FROM WHERE AND OR NOT IN IS NULL LIKE BETWEEN EXISTS JOIN LEFT RIGHT INNER OUTER ON AS
GROUP BY ORDER HAVING UNION ALL DISTINCT INSERT INTO VALUES UPDATE SET DELETE CREATE TABLE VIEW INDEX DROP ALTER CASE WHEN
THEN ELSE END ASC DESC LIMIT WITH`.split(/\s+/)

/**
 * 轻量补全（解析器不可用时，M1b2 设计文档第 7 节）：常用关键字；光标前的 `a.` / `s.t.` 作为限定名；
 * 用正则找 FROM / JOIN / UPDATE / INTO 后的表和别名。
 */
export function lightSuggest(text: string, offset: number): Suggest {
  const before = text.slice(0, offset)
  const qm = /((?:[\w$#]+|"[^"]+"|`[^`]+`)\.){1,2}[\w$#]*$/u.exec(before)
  const qualifier = qm
    ? qm[0]
        .split('.')
        .slice(0, -1)
        .map((s) => s.replace(/^["`]|["`]$/g, ''))
    : []
  const tables: TableRef[] = []
  const re = /\b(?:FROM|JOIN|UPDATE|INTO)\s+((?:[\w$#]+\.)?[\w$#]+)(?:\s+(?:AS\s+)?([\w$#]+))?/giu
  for (const m of text.matchAll(re)) {
    const parts = m[1].split('.')
    const ref: TableRef = parts.length === 2 ? { schema: parts[0], name: parts[1] } : { name: parts[0] }
    const alias = m[2]
    if (alias && !COMMON_KEYWORDS.includes(alias.toUpperCase())) ref.alias = alias
    tables.push(ref)
  }
  return { keywords: qualifier.length ? [] : COMMON_KEYWORDS, want: ['schema', 'table', 'view', 'column', 'function'], qualifier, tables }
}
