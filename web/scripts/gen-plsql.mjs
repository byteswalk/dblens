// 生成 PL/SQL 解析器（Oracle、达梦；M1b2 设计文档第 6 节）：grammars/plsql 中的上游语法 + 本文件的补丁，
// 用 antlr-ng 生成 TypeScript 到 src/features/sql/plsql/gen/。生成结果不提交；输入不变时跳过。
//
// 用法：node scripts/gen-plsql.mjs [--force]

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const web = join(dirname(fileURLToPath(import.meta.url)), '..')
const src = join(web, 'grammars/plsql')
const out = join(web, 'src/features/sql/plsql/gen')
const antlrNg = join(web, 'node_modules/antlr-ng')

/**
 * 语法补丁：[说明, 原文, 替换]。原文必须在语法中恰好出现一次，否则报错（上游改动后需要调整）。
 * 每条都在 src/features/sql/plsql/parser.test.ts 中有对应的语句。
 */
export const PATCHES = [
  ['DROP FUNCTION IF EXISTS（达梦、Oracle 23ai）', "    : DROP FUNCTION function_name\n", "    : DROP FUNCTION (IF EXISTS)? function_name\n"],
  [
    'DROP PACKAGE IF EXISTS',
    "    : DROP PACKAGE BODY? (schema_object_name '.')? package_name\n",
    "    : DROP PACKAGE BODY? (IF EXISTS)? (schema_object_name '.')? package_name\n",
  ],
  ['DROP PROCEDURE IF EXISTS', "    : DROP PROCEDURE procedure_name\n", "    : DROP PROCEDURE (IF EXISTS)? procedure_name\n"],
  ['DROP TRIGGER IF EXISTS', "    : DROP TRIGGER trigger_name\n", "    : DROP TRIGGER (IF EXISTS)? trigger_name\n"],
  ['DROP SEQUENCE IF EXISTS', "    : DROP SEQUENCE sequence_name\n", "    : DROP SEQUENCE (IF EXISTS)? sequence_name\n"],
  [
    'DROP MATERIALIZED VIEW IF EXISTS',
    "    : DROP MATERIALIZED VIEW tableview_name (PRESERVE TABLE)?\n",
    "    : DROP MATERIALIZED VIEW (IF EXISTS)? tableview_name (PRESERVE TABLE)?\n",
  ],
  [
    'DROP TABLE IF EXISTS 写在表名前（上游只支持写在表名后）',
    '    : DROP TABLE tableview_name (IF EXISTS)?',
    '    : DROP TABLE (IF EXISTS)? tableview_name (IF EXISTS)?',
  ],
  ['DROP VIEW IF EXISTS 写在视图名前', '    : DROP VIEW tableview_name (IF EXISTS)?', '    : DROP VIEW (IF EXISTS)? tableview_name (IF EXISTS)?'],
  ['DROP SYNONYM IF EXISTS', "    : DROP PUBLIC? SYNONYM (schema_name '.')?", "    : DROP PUBLIC? SYNONYM (IF EXISTS)? (schema_name '.')?"],
  [
    '达梦：DROP TABLE … CASCADE（不带 CONSTRAINTS）',
    '(CASCADE (CONSTRAINT | CONSTRAINTS))? PURGE?',
    '(CASCADE (CONSTRAINT | CONSTRAINTS)?)? PURGE?',
  ],
  [
    '达梦：自增列 IDENTITY(种子, 增量)',
    '    : GENERATED (ALWAYS | BY DEFAULT (ON NULL_)?)? AS IDENTITY identity_options_parentheses?\n',
    "    : GENERATED (ALWAYS | BY DEFAULT (ON NULL_)?)? AS IDENTITY identity_options_parentheses?\n    | IDENTITY ('(' expression ',' expression ')')?\n",
  ],
  ['达梦：COMMENT ON VIEW', '    : COMMENT ON TABLE tableview_name IS quoted_string\n', '    : COMMENT ON (TABLE | VIEW) tableview_name IS quoted_string\n'],
  [
    '达梦：BIGINT、TINYINT、BYTE、TEXT、TIME 类型（词法中有这些词，语法的类型里没有）',
    '    | XMLTYPE\n    ;',
    '    | XMLTYPE\n    | BIGINT\n    | TINYINT\n    | BYTE\n    | TEXT\n    | TIME\n    ;',
  ],
  [
    '达梦：VARBINARY(n) 等以名称出现的类型带长度',
    '    : column_name ((datatype | type_name) (COLLATE',
    '    : column_name ((datatype | type_name precision_part?) (COLLATE',
  ],
  [
    "分区边界中的 DATE 字面量：VALUES LESS THAN (DATE '2024-01-01')（Oracle、达梦）",
    "range_values_list\n    : literal (',' literal)*\n    | TIMESTAMP literal (',' TIMESTAMP literal)*\n",
    "range_values_list\n    : literal (',' literal)*\n    | TIMESTAMP literal (',' TIMESTAMP literal)*\n    | DATE literal (',' DATE literal)*\n",
  ],
]

/** 按上游 Antlr4ng/transformGrammar.py 插入基类的 import。 */
const HEADERS = [
  ['// Insert here @header for C++ lexer.', '@header {import { PlSqlLexerBase } from "./PlSqlLexerBase.js"}'],
  ['// Insert here @header for C++ parser.', '@header {import { PlSqlParserBase } from "./PlSqlParserBase.js"}'],
]

function replaceOnce(text, from, to, what) {
  const n = text.split(from).length - 1
  if (n !== 1) throw new Error(`补丁「${what}」：原文出现 ${n} 次，应为 1 次`)
  return text.replace(from, () => to)
}

export function patchGrammar(name, text) {
  for (const [from, to] of HEADERS) if (text.includes(from)) text = text.replace(from, to)
  if (name === 'PlSqlParser.g4') for (const [what, from, to] of PATCHES) text = replaceOnce(text, from, to, what)
  return text
}

const FILES = ['PlSqlLexer.g4', 'PlSqlParser.g4', 'PlSqlLexerBase.ts', 'PlSqlParserBase.ts']

function inputsHash() {
  const h = createHash('sha256')
  h.update(readFileSync(fileURLToPath(import.meta.url)))
  h.update(readFileSync(join(antlrNg, 'package.json')))
  for (const f of FILES) h.update(readFileSync(join(src, f)))
  return h.digest('hex')
}

function main() {
  const hash = inputsHash()
  const stamp = join(out, '.inputs-hash')
  if (!process.argv.includes('--force') && existsSync(stamp) && readFileSync(stamp, 'utf8') === hash) return
  const started = Date.now()
  rmSync(out, { recursive: true, force: true })
  mkdirSync(out, { recursive: true })
  for (const f of FILES) {
    const text = readFileSync(join(src, f), 'utf8')
    writeFileSync(join(out, f), f.endsWith('.g4') ? patchGrammar(f, text) : `// @ts-nocheck\n${text}`)
  }
  execFileSync(
    process.execPath,
    [join(antlrNg, 'dist/cli/runner.js'), '-Dlanguage=TypeScript', '--generate-listener', 'false', '-o', '.', 'PlSqlLexer.g4', 'PlSqlParser.g4'],
    { cwd: out, stdio: ['ignore', 'ignore', 'pipe'] },
  )
  // 生成的代码不按本项目的 tsconfig 检查（未使用的变量等）
  for (const f of ['PlSqlLexer.ts', 'PlSqlParser.ts']) {
    const p = join(out, f)
    writeFileSync(p, `// @ts-nocheck\n${readFileSync(p, 'utf8')}`)
  }
  writeFileSync(stamp, hash)
  console.log(`已生成 PL/SQL 解析器（${((Date.now() - started) / 1000).toFixed(1)} 秒）`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
