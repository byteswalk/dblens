// Oracle、达梦的语言服务（M1b2 设计文档第 6 节）：grammars-v4 的 PL/SQL 语法（打过达梦补丁）生成的解析器，
// 加 antlr4-c3 分析光标处可以出现的规则和关键字。做法参照 dt-sql-parser：
// - 先分句，再逐句解析：一句出错不影响后面的检查；补全只解析光标所在的一句；没改动的句子用缓存；
// - 先用 SLL 解析，出错再用 LL 重新解析（SLL 快，但个别语句会误报）。
// 分句规则与后端 dblens-dialect 的 split.rs 一致：`;` 结束语句；PL/SQL 块以单独一行的 `/` 结束。

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
import { PlSqlLexer } from './gen/PlSqlLexer'
import { PlSqlParser, type Sql_scriptContext } from './gen/PlSqlParser'

const P = PlSqlParser

/** 把 ANTLR 的报错转成 ParseIssue（行列相对于被解析的那一句）。 */
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
      // 在末尾出错（ANTLR 有时报在最后一个词上）：语句不完整，标出最后一个词
      this.issues.push({ ...tokenRange(last, line, column), incomplete: true })
    } else if (offending) {
      this.issues.push({ ...tokenRange(offending, line, column), near: offending.text ?? '' })
    } else {
      // 词法错误（如字符串没有结束）：msg 形如 token recognition error at: '...'
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

function lex(sql: string, listener?: IssueCollector): Token[] {
  const lexer = new PlSqlLexer(CharStream.fromString(sql))
  lexer.removeErrorListeners()
  if (listener) lexer.addErrorListener(listener)
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  return stream.getTokens()
}

const isCode = (t: Token) => t.channel === Token.DEFAULT_CHANNEL && t.type !== Token.EOF

interface Parsed {
  tree: Sql_scriptContext
  tokens: Token[]
  parser: PlSqlParser
  /** 只保留第一处：一句中后面的报错多是连带的。 */
  issue: ParseIssue | null
}

// 按句缓存：编辑时大部分句子没变
const cache = new Map<string, Parsed>()
const CACHE_LIMIT = 500

/** 解析一句（或任意文本）。 */
export function parseText(text: string): Parsed {
  const hit = cache.get(text)
  if (hit) {
    // 移到最后：最近用过的不先被淘汰
    cache.delete(text)
    cache.set(text, hit)
    return hit
  }
  let tokens: Token[] = []
  const lastWord = () => [...tokens].reverse().find(isCode) ?? null
  const lexErrors = new IssueCollector(lastWord)
  const lexer = new PlSqlLexer(CharStream.fromString(text))
  lexer.removeErrorListeners()
  lexer.addErrorListener(lexErrors)
  const stream = new CommonTokenStream(lexer)
  stream.fill()
  tokens = stream.getTokens()
  const parser = new PlSqlParser(stream)
  const errors = new IssueCollector(lastWord)
  parser.removeErrorListeners()
  parser.addErrorListener(errors)
  parser.interpreter.predictionMode = PredictionMode.SLL
  let tree = parser.sql_script()
  if (errors.issues.length) {
    errors.issues = []
    stream.seek(0)
    parser.reset()
    parser.interpreter.predictionMode = PredictionMode.LL
    tree = parser.sql_script()
  }
  const parsed = { tree, tokens, parser, issue: lexErrors.issues[0] ?? errors.issues[0] ?? null }
  cache.set(text, parsed)
  if (cache.size > CACHE_LIMIT) cache.delete(cache.keys().next().value as string)
  return parsed
}

/** 一句在全文中的位置：[start, end) 为 UTF-16 偏移，line / column 为起点（行从 1、列从 0 开始）。 */
export interface Piece {
  start: number
  end: number
  line: number
  column: number
}

const BLOCK_OBJECTS = new Set(['PROCEDURE', 'FUNCTION', 'PACKAGE', 'TRIGGER', 'TYPE', 'LIBRARY', 'CLASS'])

/** 语句开头是否是 PL/SQL 块（与 split.rs 的 starts_plsql 一致）。 */
function startsBlock(words: string[]): boolean {
  if (words[0] === 'DECLARE' || words[0] === 'BEGIN') return true
  if (words[0] !== 'CREATE') return false
  let k = 1
  if (words[1] === 'OR' && words[2] === 'REPLACE') k = 3
  if (['EDITIONABLE', 'NONEDITIONABLE', 'EDITIONING'].includes(words[k])) k++
  return BLOCK_OBJECTS.has(words[k])
}

/** `/` 单独占一行（与 split.rs 一致）。 */
function aloneOnLine(tokens: Token[], i: number): boolean {
  const t = tokens[i]
  const near = (j: number) => tokens[j] && tokens[j].type !== Token.EOF && tokens[j].line === t.line && tokens[j].channel === Token.DEFAULT_CHANNEL
  for (let j = i - 1; j >= 0 && tokens[j].line === t.line; j--) if (near(j)) return false
  for (let j = i + 1; j < tokens.length && tokens[j].line === t.line; j++) if (near(j)) return false
  return true
}

/** 分句：结束符（`;`、`/`）不含在句中。 */
export function split(tokens: Token[]): Piece[] {
  const out: Piece[] = []
  let first: Token | null = null
  let words: string[] = []
  const close = (end: number) => {
    if (first) out.push({ start: first.start, end, line: first.line, column: first.column })
    first = null
    words = []
  }
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]
    if (!isCode(t)) continue
    if (t.type === P.SOLIDUS && aloneOnLine(tokens, i)) {
      close(t.start)
      continue
    }
    if (!first) first = t
    if (words.length < 6) words.push((t.text ?? '').toUpperCase())
    if (t.type === P.SEMICOLON && !startsBlock(words)) close(t.start)
  }
  if (first) close((tokens[tokens.length - 1]?.start ?? 0))
  return out
}

/** 把一句中的行列换算成全文中的行列。 */
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

const WORD = /^[\p{L}_$#"]/u

/** 光标对应的 token 下标（c3 从这里给候选）：光标在词中或紧贴词尾时是这个词，否则是光标后的第一个 token。 */
export function caretTokenIndex(tokens: Token[], offset: number): number {
  for (const t of tokens) {
    if (t.channel !== Token.DEFAULT_CHANNEL && t.type !== Token.EOF) continue
    if (t.type === Token.EOF || t.start >= offset) return t.tokenIndex
    if (offset <= t.stop + 1 && WORD.test(t.text ?? '')) return t.tokenIndex
  }
  return tokens.length - 1
}

function unquote(s: string): string {
  return s.startsWith('"') && s.endsWith('"') ? s.slice(1, -1).replace(/""/g, '"') : s
}

const isName = (t: Token | undefined) => !!t && isCode(t) && WORD.test(t.text ?? '')

/** 光标前的限定名：`hr.emp.` → ['hr', 'emp']；光标在词中时不算这个词。 */
function qualifierBefore(code: Token[], pos: number): string[] {
  const out: string[] = []
  let i = pos - 1
  while (i >= 1 && code[i].type === P.PERIOD && isName(code[i - 1])) {
    out.unshift(unquote(code[i - 1].text ?? ''))
    i -= 2
  }
  return out
}

/** 表名后面的这些词不是别名。 */
const NOT_ALIAS = new Set(
  'WHERE JOIN ON USING INNER LEFT RIGHT FULL OUTER CROSS NATURAL GROUP ORDER HAVING CONNECT START UNION MINUS INTERSECT EXCEPT SET VALUES WHEN FETCH OFFSET FOR PARTITION SAMPLE PIVOT UNPIVOT AS OF VERSIONS SELECT WITH RETURNING RETURN LOG MODEL WINDOW QUALIFY LIMIT'.split(' '),
)
/** 这些词后面是表名。 */
const BEFORE_TABLE = new Set([P.FROM, P.JOIN, P.INTO, P.UPDATE, P.USING, P.TABLE])

/**
 * 按词扫描语句中的表和别名：FROM / JOIN / INTO / UPDATE / USING / TABLE 后的名称，FROM 后逗号分隔的多个表。
 * 不依赖语法树，写到一半的语句也能识别。
 */
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
      while (code[j]?.type === P.PERIOD && isName(code[j + 1])) {
        parts.push(unquote(code[j + 1].text ?? ''))
        j += 2
      }
      if (code[j]?.type === P.AT_SIGN && isName(code[j + 1])) j += 2 // 数据库链接
      let alias: string | undefined
      if (code[j]?.type === P.AS && isName(code[j + 1])) j++
      if (isName(code[j]) && !NOT_ALIAS.has((code[j].text ?? '').toUpperCase())) alias = unquote(code[j++].text ?? '')
      if (parts.length <= 2 && !NOT_ALIAS.has(parts[parts.length - 1].toUpperCase())) {
        const ref: TableRef = parts.length === 2 ? { schema: parts[0], name: parts[1] } : { name: parts[0] }
        if (alias) ref.alias = alias
        const key = JSON.stringify(ref)
        if (!seen.has(key)) {
          seen.add(key)
          out.push(ref)
        }
      }
      // FROM a, b：逗号后还是表
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
  [P.RULE_tableview_name, ['table', 'view']],
  [P.RULE_general_element, ['column', 'function']],
  [P.RULE_column_name, ['column']],
  [P.RULE_schema_name, ['schema']],
  [P.RULE_function_name, ['function']],
  // 只用来挡住标识符位置上大量的非保留关键字，不产生补全
  [P.RULE_regular_id, []],
  [P.RULE_identifier, []],
])

const KEYWORD = /^[A-Z][A-Z0-9_]*$/

export const plsql: SqlParser = {
  validate(sql) {
    const out: ParseIssue[] = []
    for (const p of split(lex(sql))) {
      const { issue } = parseText(sql.slice(p.start, p.end))
      if (issue) out.push(shift(issue, p))
    }
    return out
  },

  suggest(sql, offset): Suggest {
    // 光标所在的一句；光标在两句之间（如结束符之后）时是一句新的空语句
    const piece = split(lex(sql)).find((p) => p.start <= offset && offset <= p.end) ?? { start: offset, end: offset }
    const { tree, tokens, parser } = parseText(sql.slice(piece.start, piece.end))
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
    // 句末的空白不算在语句里
    const ranges = split(lex(sql)).map((p) => ({ start: p.start, end: p.start + sql.slice(p.start, p.end).trimEnd().length }))
    return pickStatement(ranges, offset)
  },
}
