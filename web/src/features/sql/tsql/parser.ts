// SQL Server 的语言服务（M6a 设计文档第 8 节）：grammars-v4 的 T-SQL 语法生成的解析器，加 antlr4-c3 分析光标处
// 可以出现的规则和关键字。做法同 PL/SQL（plsql/parser.ts）：先分批再逐批解析，没改动的批用缓存；先 SLL 后 LL。
// 分批规则与后端 dblens-dialect 的 split.rs 一致：单独一行的 `GO [次数]`（行尾可有注释）结束一批；
// 字符串、`[…]`、`"…"` 和注释里的 GO 由词法分析排除。"光标处的语句"就是光标所在的一批。

import { CodeCompletionCore } from 'antlr4-c3'
import {
  type ATNSimulator,
  BaseErrorListener,
  CharStream,
  CommonTokenStream,
  PredictionMode,
  type Recognizer,
  Token,
} from 'antlr4ng'

import { type ParseIssue, type SqlParser, type Suggest, type TableRef, type Want, pickStatement } from '../types'
import { TSqlLexer } from './gen/TSqlLexer'
import { TSqlParser, type Tsql_fileContext } from './gen/TSqlParser'

const P = TSqlParser

/** 把 ANTLR 的报错转成 ParseIssue（行列相对于被解析的那一批）。 */
class IssueCollector extends BaseErrorListener {
  issues: ParseIssue[] = []
  constructor(private readonly lastWord: () => Token | null) {
    super()
  }

  override syntaxError<S extends Token, T extends ATNSimulator>(
    _r: Recognizer<T>,
    offending: S | null,
    line: number,
    column: number,
    msg: string,
  ): void {
    const last = this.lastWord()
    if (offending && (offending.type === Token.EOF || offending.tokenIndex === last?.tokenIndex)) {
      this.issues.push({ ...tokenRange(last, line, column), incomplete: true })
    } else if (offending) {
      this.issues.push({ ...tokenRange(offending, line, column), near: offending.text ?? '' })
    } else {
      const near = /at: '(.*)'$/s.exec(msg)?.[1] ?? ''
      const first = near.split('\n')[0]
      this.issues.push({ startLine: line, startColumn: column + 1, endLine: line, endColumn: column + 1 + Math.max(first.length, 1), near: first })
    }
  }
}

function tokenRange(t: Token | null, line: number, column: number) {
  if (!t || t.type === Token.EOF) return { startLine: line, startColumn: column + 1, endLine: line, endColumn: column + 2 }
  const lines = (t.text ?? '').split('\n')
  const endLine = t.line + lines.length - 1
  const endColumn = lines.length > 1 ? lines[lines.length - 1].length + 1 : t.column + 1 + lines[0].length
  return { startLine: t.line, startColumn: t.column + 1, endLine, endColumn }
}

function lex(sql: string): Token[] {
  const lexer = new TSqlLexer(CharStream.fromString(sql))
  lexer.removeErrorListeners()
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  return stream.getTokens()
}

const isCode = (t: Token) => t.channel === Token.DEFAULT_CHANNEL && t.type !== Token.EOF

interface Parsed {
  tree: Tsql_fileContext
  tokens: Token[]
  parser: TSqlParser
  /** 只保留第一处：一批中后面的报错多是连带的。 */
  issue: ParseIssue | null
}

const cache = new Map<string, Parsed>()
const CACHE_LIMIT = 500

/** 解析一批（或任意文本）。 */
export function parseText(text: string): Parsed {
  const hit = cache.get(text)
  if (hit) {
    cache.delete(text)
    cache.set(text, hit)
    return hit
  }
  let tokens: Token[] = []
  const lastWord = () => [...tokens].reverse().find(isCode) ?? null
  const lexErrors = new IssueCollector(lastWord)
  const lexer = new TSqlLexer(CharStream.fromString(text))
  lexer.removeErrorListeners()
  lexer.addErrorListener(lexErrors)
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  tokens = stream.getTokens()
  const parser = new TSqlParser(stream)
  const errors = new IssueCollector(lastWord)
  parser.removeErrorListeners()
  parser.addErrorListener(errors)
  parser.interpreter.predictionMode = PredictionMode.SLL
  let tree = parser.tsql_file()
  if (errors.issues.length) {
    errors.issues = []
    stream.seek(0)
    parser.reset()
    parser.interpreter.predictionMode = PredictionMode.LL
    tree = parser.tsql_file()
  }
  const parsed = { tree, tokens, parser, issue: lexErrors.issues[0] ?? errors.issues[0] ?? null }
  cache.set(text, parsed)
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string)
  return parsed
}

/**
 * 一批在全文中的位置：[start, end) 为 UTF-16 偏移（不含批前后的空白和注释），line / column 为起点（行从 1、列从 0 开始）；
 * `limit` 是这一批能延伸到的位置（下一个 GO 行的开头或全文末尾），光标在 [start, limit] 中都算在这一批里。
 */
export interface Piece {
  start: number
  end: number
  line: number
  column: number
  limit: number
}

/** 第 i 个 token 是否是批的结束：单独一行的 GO，后面同一行只能有次数（注释在隐藏通道上）。 */
function isGoLine(tokens: Token[], i: number): boolean {
  const t = tokens[i]
  if (t.type !== P.GO) return false
  const sameLine = (j: number) => tokens[j] && tokens[j].line === t.line && isCode(tokens[j])
  for (let j = i - 1; j >= 0 && tokens[j].line === t.line; j--) if (sameLine(j)) return false
  const after: Token[] = []
  for (let j = i + 1; j < tokens.length && tokens[j].line === t.line; j++) if (sameLine(j)) after.push(tokens[j])
  return after.length === 0 || (after.length === 1 && after[0].type === P.DECIMAL)
}

/** 分批：GO 行不含在批中；没有代码的批跳过。 */
export function split(tokens: Token[]): Piece[] {
  const out: Piece[] = []
  let first: Token | null = null
  let last: Token | null = null
  const close = (limit: number) => {
    if (first && last) out.push({ start: first.start, end: last.stop + 1, line: first.line, column: first.column, limit })
    first = null
    last = null
  }
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]
    if (!isCode(t)) continue
    if (isGoLine(tokens, i)) {
      close(t.start)
      // 跳过同一行的次数
      while (tokens[i + 1] && tokens[i + 1].line === t.line) i++
      continue
    }
    if (!first) first = t
    last = t
  }
  close(tokens[tokens.length - 1]?.start ?? 0)
  return out
}

function shift(i: ParseIssue, p: Piece): ParseIssue {
  const col = (line: number, c: number) => (line === 1 ? c + p.column : c)
  return {
    ...i,
    startLine: i.startLine + p.line - 1,
    startColumn: col(i.startLine, i.startColumn),
    endLine: i.endLine + p.line - 1,
    endColumn: col(i.endLine, i.endColumn),
  }
}

const WORD = /^[\p{L}_#@["]/u

/** 光标对应的 token 下标（c3 从这里给候选）：光标在词中或紧贴词尾时是这个词，否则是光标后的第一个 token。 */
export function caretTokenIndex(tokens: Token[], offset: number): number {
  for (const t of tokens) {
    if (t.channel !== Token.DEFAULT_CHANNEL && t.type !== Token.EOF) continue
    if (t.type === Token.EOF || t.start >= offset) return t.tokenIndex
    if (offset <= t.stop + 1 && WORD.test(t.text ?? '')) return t.tokenIndex
  }
  return tokens.length - 1
}

/** 去掉方括号或双引号。 */
export function unquote(s: string): string {
  if (s.startsWith('[') && s.endsWith(']')) return s.slice(1, -1).replace(/\]\]/g, ']')
  if (s.startsWith('"') && s.endsWith('"')) return s.slice(1, -1).replace(/""/g, '"')
  return s
}

const isName = (t: Token | undefined) => !!t && isCode(t) && WORD.test(t.text ?? '') && !(t.text ?? '').startsWith('@')

function qualifierBefore(code: Token[], pos: number): string[] {
  const out: string[] = []
  let i = pos - 1
  while (i >= 1 && code[i].type === P.DOT && isName(code[i - 1])) {
    out.unshift(unquote(code[i - 1].text ?? ''))
    i -= 2
  }
  return out
}

/** 表名后面的这些词不是别名。 */
const NOT_ALIAS = new Set(
  'WHERE JOIN ON INNER LEFT RIGHT FULL OUTER CROSS APPLY GROUP ORDER HAVING UNION EXCEPT INTERSECT SET VALUES WHEN OPTION FOR PIVOT UNPIVOT AS WITH OUTPUT SELECT TABLESAMPLE DEFAULT GO'.split(' '),
)
/** 这些词后面是表名。 */
const BEFORE_TABLE = new Set([P.FROM, P.JOIN, P.INTO, P.UPDATE, P.TABLE, P.MERGE])

/** 按词扫描语句中的表和别名（三段名 `库.架构.表` 取后两段）。不依赖语法树，写到一半的语句也能识别。 */
export function scanTables(code: Token[]): TableRef[] {
  const out: TableRef[] = []
  const seen = new Set<string>()
  for (let i = 0; i < code.length; i++) {
    if (!BEFORE_TABLE.has(code[i].type)) continue
    let j = i + 1
    for (;;) {
      if (!isName(code[j])) break
      const parts = [unquote(code[j].text ?? '')]
      j++
      while (code[j]?.type === P.DOT && isName(code[j + 1])) {
        parts.push(unquote(code[j + 1].text ?? ''))
        j += 2
      }
      let alias: string | undefined
      if (code[j]?.type === P.AS && isName(code[j + 1])) j++
      if (isName(code[j]) && !NOT_ALIAS.has((code[j].text ?? '').toUpperCase())) alias = unquote(code[j++].text ?? '')
      const name = parts[parts.length - 1]
      if (parts.length <= 3 && !NOT_ALIAS.has(name.toUpperCase())) {
        const ref: TableRef = parts.length >= 2 ? { schema: parts[parts.length - 2], name } : { name }
        if (alias) ref.alias = alias
        const key = JSON.stringify(ref)
        if (!seen.has(key)) {
          seen.add(key)
          out.push(ref)
        }
      }
      if (code[i].type === P.FROM && code[j]?.type === P.COMMA) {
        j++
        continue
      }
      break
    }
  }
  return out
}

const PREFERRED = new Map<number, Want[]>([
  [P.RULE_full_table_name, ['table', 'view']],
  [P.RULE_table_name, ['table', 'view']],
  [P.RULE_full_column_name, ['column']],
  [P.RULE_scalar_function_name, ['function']],
  // 只用来挡住标识符位置上大量的非保留关键字，不产生补全
  [P.RULE_id_, []],
  [P.RULE_keyword, []],
])

const KEYWORD = /^[A-Z][A-Z0-9_]*$/

export const tsql: SqlParser = {
  validate(sql) {
    const out: ParseIssue[] = []
    for (const p of split(lex(sql))) {
      const { issue } = parseText(sql.slice(p.start, p.end))
      if (issue) out.push(shift(issue, p))
    }
    return out
  },

  suggest(sql, offset): Suggest {
    // 光标所在的一批；光标在批之外（如 GO 之后的空行）时是新的空批
    const piece = split(lex(sql)).find((p) => p.start <= offset && offset <= p.limit) ?? { start: offset, limit: offset }
    const { tree, tokens, parser } = parseText(sql.slice(piece.start, piece.limit))
    const caret = offset - piece.start
    const idx = caretTokenIndex(tokens, caret)
    const core = new CodeCompletionCore(parser)
    core.preferredRules = new Set(PREFERRED.keys())
    const cands = core.collectCandidates(idx, tree)
    const want = new Set<Want>()
    for (const rule of cands.rules.keys()) for (const w of PREFERRED.get(rule) ?? []) want.add(w)
    const keywords: string[] = []
    for (const t of cands.tokens.keys()) {
      const name = parser.vocabulary.getLiteralName(t)?.replace(/^'|'$/g, '')
      if (name && KEYWORD.test(name)) keywords.push(name)
    }
    const code = tokens.filter(isCode)
    const pos = code.findIndex((t) => t.tokenIndex >= idx)
    return { keywords, want: [...want], qualifier: qualifierBefore(code, pos < 0 ? code.length : pos), tables: scanTables(code) }
  },

  statementAt(sql, offset) {
    const ranges = split(lex(sql)).map((p) => ({ start: p.start, end: p.end }))
    return pickStatement(ranges, offset)
  },
}
