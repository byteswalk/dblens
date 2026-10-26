// MySQL 系、PG 系的语言服务：把 dt-sql-parser 的结果转成 SqlParser 的格式。
// 具体的解析器在 mysql.ts / postgres.ts 中创建，各自是一个懒加载的块。

import type { EntityContext, MySQL, PostgreSQL, Suggestions } from 'dt-sql-parser'

import type { SqlParser, Suggest, TableRef, Want } from './types'

type DtParser = MySQL | PostgreSQL

// dt-sql-parser 的 EntityContextType 取值（只引入类型，不引入实现）
const WANT: Record<string, Want[]> = {
  table: ['table', 'view'],
  view: ['view'],
  column: ['column'],
  function: ['function'],
  database: ['schema'],
}

/** 偏移 → 行列（都从 1 开始）。 */
export function caretAt(sql: string, offset: number) {
  const before = sql.slice(0, offset).split('\n')
  return { lineNumber: before.length, column: before[before.length - 1].length + 1 }
}

function unquote(s: string): string {
  const q = s[0]
  return (q === '`' || q === '"') && s.endsWith(q) && s.length > 1 ? s.slice(1, -1).replaceAll(q + q, q) : s
}

/** 按 `.` 拆分名称，引号中的 `.` 不拆。 */
function splitName(text: string): string[] {
  return (text.match(/`(?:[^`]|``)*`|"(?:[^"]|"")*"|[^.]+/g) ?? []).map(unquote)
}

/** wordRanges 是光标前已写的部分，如 ['e', '.', 'na']：最后一个不是 `.` 时是正在输入的词，不算限定名。 */
function qualifierOf(words: string[]): string[] {
  const parts = words[words.length - 1] === '.' ? words : words.slice(0, -1)
  if (parts.some((w, i) => (i % 2 === 1) !== (w === '.'))) return []
  return parts.filter((w) => w !== '.').map(unquote)
}

function tablesOf(entities: EntityContext[] | null): TableRef[] {
  const out: TableRef[] = []
  for (const e of entities ?? []) {
    if (!e.belongStmt.isContainCaret || (e.entityContextType !== 'table' && e.entityContextType !== 'view')) continue
    const parts = splitName(e.text)
    if (parts.length < 1 || parts.length > 2) continue
    const ref: TableRef = parts.length === 2 ? { schema: parts[0], name: parts[1] } : { name: parts[0] }
    const alias = e._alias?.text
    if (alias) ref.alias = unquote(alias)
    out.push(ref)
  }
  return out
}

/** dt-sql-parser 补全时的中间解析会把报错打印到控制台，这里不需要。 */
function quiet<T>(fn: () => T): T {
  const error = console.error
  console.error = () => {}
  try {
    return fn()
  } finally {
    console.error = error
  }
}

export function adapt(p: DtParser): SqlParser {
  return {
    validate(sql, locale) {
      p.locale = locale
      return quiet(() => p.validate(sql)).map((e) => ({
        startLine: e.startLine,
        startColumn: e.startColumn,
        endLine: e.endLine,
        endColumn: e.endColumn,
        message: e.message,
      }))
    },
    suggest(sql, offset): Suggest {
      const caret = caretAt(sql, offset)
      const s: Suggestions | null = quiet(() => p.getSuggestionAtCaretPosition(sql, caret))
      const want = new Set<Want>()
      for (const x of s?.syntax ?? []) for (const w of WANT[x.syntaxContextType] ?? []) want.add(w)
      const words = s?.syntax[0]?.wordRanges.map((w) => w.text) ?? []
      return {
        keywords: s?.keywords ?? [],
        want: [...want],
        qualifier: qualifierOf(words),
        tables: tablesOf(quiet(() => p.getAllEntities(sql, caret))),
      }
    },
  }
}
