// SQL 语言服务（M1b2 设计文档第 4 节）的公共类型：worker 与主线程之间传递的数据。

/** 解析器的语法族：MySQL 系、PG 系用 dt-sql-parser，Oracle 和达梦用 PL/SQL 语法。 */
export type Dialect = 'mysql' | 'postgres' | 'plsql'

/** 错误文字的语言（dt-sql-parser 的取值）。 */
export type ParserLocale = 'zh_CN' | 'en_US'

/** 一处语法问题。行列从 1 开始，结束列不含。 */
export interface ParseIssue {
  startLine: number
  startColumn: number
  endLine: number
  endColumn: number
  /** 解析器给出的文字（dt-sql-parser 已按语言翻译）。没有时由主线程按 incomplete / near 生成。 */
  message?: string
  /** 语句不完整（在末尾出错）。 */
  incomplete?: boolean
  /** 出错位置的词。 */
  near?: string
}

/** 光标处该出现的对象。 */
export type Want = 'schema' | 'table' | 'view' | 'column' | 'function'

/** 语句中出现的表（含视图）。 */
export interface TableRef {
  /** 写在语句里的限定名，如 `hr`；没写时为空。 */
  schema?: string
  name: string
  alias?: string
}

/** worker 对光标处的分析：主线程据此查元数据、生成补全项。 */
export interface Suggest {
  /** 解析器认为可以出现的关键字（大写）。 */
  keywords: string[]
  want: Want[]
  /** 光标前已写的限定名，如 `a.` → ['a']，`hr.emp.` → ['hr', 'emp']。 */
  qualifier: string[]
  /** 光标所在语句中出现的表。 */
  tables: TableRef[]
}

/** 一条语句在全文中的范围：[start, end) 为 UTF-16 偏移，不含结束符。 */
export interface StatementRange {
  start: number
  end: number
}

/** 语言服务：每种 Dialect 一个实现，在 worker 里运行（测试中直接调用）。 */
export interface SqlParser {
  validate(sql: string, locale: ParserLocale): ParseIssue[]
  /** offset：光标在 sql 中的 UTF-16 偏移。 */
  suggest(sql: string, offset: number): Suggest
  /** 光标所在的语句（规则同后端 `statement_at`：取开始位置不在光标之后的最后一条，光标在第一条之前时取第一条）；没有语句时为 null。 */
  statementAt(sql: string, offset: number): StatementRange | null
}

/** 从按位置排好的语句中选出光标所在的一条（规则见 `SqlParser.statementAt`）。 */
export function pickStatement(ranges: StatementRange[], offset: number): StatementRange | null {
  return [...ranges].reverse().find((r) => r.start <= offset) ?? ranges[0] ?? null
}
