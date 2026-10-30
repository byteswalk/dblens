// 生成 T-SQL 解析器（SQL Server；M6a 设计文档第 8 节）：grammars/tsql 中的上游语法，原样用 antlr-ng 生成 TypeScript
// 到 src/features/sql/tsql/gen/。生成结果不提交；输入不变时跳过。
//
// 用法：node scripts/gen-tsql.mjs [--force]

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const web = join(dirname(fileURLToPath(import.meta.url)), '..')
const src = join(web, 'grammars/tsql')
const out = join(web, 'src/features/sql/tsql/gen')
const antlrNg = join(web, 'node_modules/antlr-ng')

const FILES = ['TSqlLexer.g4', 'TSqlParser.g4']

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
  for (const f of FILES) writeFileSync(join(out, f), readFileSync(join(src, f), 'utf8'))
  execFileSync(
    process.execPath,
    [join(antlrNg, 'dist/cli/runner.js'), '-Dlanguage=TypeScript', '--generate-listener', 'false', '-o', '.', ...FILES],
    { cwd: out, stdio: ['ignore', 'ignore', 'pipe'] },
  )
  // 生成的代码不按本项目的 tsconfig 检查（未使用的变量等）
  for (const f of ['TSqlLexer.ts', 'TSqlParser.ts']) {
    const p = join(out, f)
    writeFileSync(p, `// @ts-nocheck\n${readFileSync(p, 'utf8')}`)
  }
  writeFileSync(stamp, hash)
  console.log(`已生成 T-SQL 解析器（${((Date.now() - started) / 1000).toFixed(1)} 秒）`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
