# M1b2 SQL 补全与语法检查 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SQL 编辑器按光标位置补全关键字、schema、表、视图、列（含别名）和函数，并在编辑时标出语法问题；MySQL 系、PG 系、Oracle、达梦都支持。

**Architecture:** 一个 Web Worker 承担解析：MySQL 系、PG 系用 dt-sql-parser，Oracle 和达梦用 grammars-v4 的 PL/SQL 语法（打达梦补丁）经 antlr-ng 生成的解析器，各自懒加载。worker 只回答"光标处该出现什么、语句里有哪些表"和"哪里有语法问题"；主线程查 react-query 缓存中的元数据生成补全项，把语法问题按档位标为红色或黄色。

**Tech Stack:** React 19 + TypeScript、Monaco 0.57、dt-sql-parser 4.5.1、antlr4ng 3.0.16、antlr4-c3 3.4.4、antlr-ng 1.0.10（开发依赖）、@tanstack/react-query 5、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`

## Global Constraints

- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级，严重安全修复不等；不能升级的在说明文档中记原因。
- 没有发布版的来源（grammars-v4）固定到提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`。
- 档位：变体 mysql、mariadb、postgres、oracle 为标准档（错误，红）；其余变体和未知变体为兼容档（警告，黄，并说明按 MySQL / PostgreSQL / Oracle 语法检查）。
- 语法检查不阻止执行；与执行出错的标记来源不同，互不覆盖。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；控制台日志不翻译。
- 解析器、Monaco 都懒加载，不进入首屏。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 执行说明

每个任务的代码都已在临时副本中逐任务实现并通过检查（任务 2 同时完成了 Oracle / 达梦的技术验证），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`web/pnpm-lock.yaml` 不列出，由任务中的命令生成；上游语法文件由任务 2 的下载步骤取得。

## 文件结构

```text
scripts/check-deps.mjs                     依赖复查（新增）
web/grammars/plsql/                        上游 PL/SQL 语法与来源说明（新增）
web/scripts/gen-plsql.mjs                  打补丁、生成 PL/SQL 解析器（新增）
web/src/features/sql/                      SQL 语言服务（新增）：
  types.ts · protocol.ts                     公共类型、worker 消息
  plsql/parser.ts · dtsql.ts · mysql.ts · postgres.ts · parsers.ts · worker.ts   解析（worker 内）
  client.ts · service.ts · dialects.ts       主线程客户端、方言与档位
  metadata.ts · completion.ts · functions.ts · provider.ts   补全
  syntax.ts · diagnostics.ts · checker.ts    语法检查
web/src/features/editor/{SqlEditor.tsx, QueryView.tsx, commands.ts}   接入编辑器
web/src/app/{StatusBar.tsx, SettingsDialog.tsx}                    状态栏、设置
web/src/features/tree/ObjectTree.tsx       刷新时补全元数据失效
web/vite.config.ts                         worker 输出 ES 模块
docs/notes/m1b2-completion.md              说明文档（新增）
```

---

### Task 1: 依赖复查

按依赖原则（设计文档 3.1 节）复查前端和后端的全部直接依赖：新增复查脚本，列出发布满 7 天的最新正式版；可升级的升级并跑全量测试。

**Files:**
- Create: `scripts/check-deps.mjs`
- Modify: `web/package.json`
- 由命令更新：`web/pnpm-lock.yaml`

**Interfaces:**
- Produces：`scripts/check-deps.mjs`（`node scripts/check-deps.mjs [--days 7]`，环境变量 `NPM_REGISTRY` 指定 npm 仓库）：每行一个依赖，`a -> b` 表示可升级，"未满 7 天"表示暂不升级，最后一行是汇总。

- [ ] **Step 1: 新建 `scripts/check-deps.mjs`**

```js
// 依赖复查（依赖原则见 docs/superpowers/specs/2026-10-25-m1b2-completion-design.md 3.1 节）：
// 列出前端（web/package.json）和后端（Cargo.toml 的 workspace.dependencies）直接依赖中
// 可以升级的项。只看正式版（版本号不带 `-`、未撤回），发布满 N 天（默认 7）才算可升级。
//
// 用法：node scripts/check-deps.mjs [--days 7]
// 环境变量 NPM_REGISTRY 指定 npm 仓库（默认 https://registry.npmjs.org，国内可用 https://registry.npmmirror.com）。

import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const daysArg = process.argv.indexOf('--days')
const days = daysArg > 0 ? Number(process.argv[daysArg + 1]) : 7
const cutoff = Date.now() - days * 86_400_000
const npmRegistry = (process.env.NPM_REGISTRY ?? 'https://registry.npmjs.org').replace(/\/$/, '')

const isStable = (v) => /^\d+\.\d+\.\d+$/.test(v)
const cmp = (a, b) => {
  const x = a.split('.').map(Number)
  const y = b.split('.').map(Number)
  for (let i = 0; i < 3; i++) if (x[i] !== y[i]) return x[i] - y[i]
  return 0
}

/** 从 [版本, 发布时间] 中选出最新的正式版和其中发布满期限的最新版。 */
function pick(versions) {
  const stable = versions.filter(([v]) => isStable(v)).sort((a, b) => cmp(b[0], a[0]))
  return { newest: stable[0], eligible: stable.find(([, time]) => Date.parse(time) <= cutoff) }
}

async function getJson(url) {
  const resp = await fetch(url, { headers: { 'User-Agent': 'dblens-check-deps (github.com/byteswalk/dblens)' } })
  if (!resp.ok) throw new Error(`${url}: HTTP ${resp.status}`)
  return resp.json()
}

async function npm(name) {
  const d = await getJson(`${npmRegistry}/${name.replace('/', '%2F')}`)
  return pick(Object.keys(d.versions).filter((v) => !d.versions[v].deprecated).map((v) => [v, d.time[v]]))
}

async function crate(name) {
  const d = await getJson(`https://crates.io/api/v1/crates/${name}/versions`)
  return pick(d.versions.filter((v) => !v.yanked).map((v) => [v.num, v.created_at]))
}

function webDeps() {
  const pkg = JSON.parse(readFileSync(join(root, 'web/package.json'), 'utf8'))
  return Object.entries({ ...pkg.dependencies, ...pkg.devDependencies }).map(([name, v]) => ({
    eco: 'npm',
    name,
    current: v.replace(/^[\^~]/, ''),
  }))
}

function cargoDeps() {
  const toml = readFileSync(join(root, 'Cargo.toml'), 'utf8')
  const section = toml.split('[workspace.dependencies]')[1].split(/^\[/m)[0]
  const deps = []
  for (const line of section.split('\n')) {
    const m = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]+)"|\{.*?version\s*=\s*"([^"]+)")/)
    if (m) deps.push({ eco: 'cargo', name: m[1], current: m[2] ?? m[3] })
  }
  return deps
}

const deps = [...webDeps(), ...cargoDeps()]
const rows = await Promise.all(
  deps.map(async (d) => {
    try {
      const { newest, eligible } = await (d.eco === 'npm' ? npm(d.name) : crate(d.name))
      return { ...d, newest, eligible }
    } catch (e) {
      return { ...d, error: e.message }
    }
  }),
)

let pending = 0
for (const r of rows) {
  if (r.error) {
    console.log(`${r.eco}\t${r.name}\t${r.current}\t查询失败：${r.error}`)
    continue
  }
  if (r.eligible && cmp(r.eligible[0], r.current) > 0) {
    pending++
    console.log(`${r.eco}\t${r.name}\t${r.current} -> ${r.eligible[0]}\t（${r.eligible[1].slice(0, 10)} 发布）`)
  } else if (r.newest && cmp(r.newest[0], r.current) > 0) {
    console.log(`${r.eco}\t${r.name}\t${r.current}\t${r.newest[0]} 未满 ${days} 天（${r.newest[1].slice(0, 10)} 发布），暂不升级`)
  }
}
console.log(`共 ${rows.length} 个直接依赖，${pending} 个可升级`)
```

- [ ] **Step 2: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -20,11 +20,11 @@
     "@tanstack/react-query": "5.104.5",
     "@tauri-apps/api": "2.12.4",
     "antd": "6.6.7",
-    "i18next": "26.4.3",
+    "i18next": "26.5.2",
     "monaco-editor": "0.57.0",
     "react": "19.3.1",
     "react-dom": "19.3.1",
-    "react-i18next": "17.0.16",
+    "react-i18next": "17.1.0",
     "react-resizable-panels": "4.14.8",
     "simple-icons": "16.35.0",
     "sql-formatter": "15.9.0",
```

- [ ] **Step 3: 依赖**

先运行复查脚本：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NPM_REGISTRY = 'https://registry.npmmirror.com'; node scripts/check-deps.mjs
```

2026-10-26 写计划时的结果：可升级 `i18next 26.4.3 -> 26.5.2`、`react-i18next 17.0.16 -> 17.1.0`；antd 6.6.8、react-resizable-panels 4.15.0、uuid（Cargo）1.29.0 未满 7 天。执行时以脚本的实际输出为准：输出中 `->` 的都升级（npm 用 `pnpm add 包@版本`，Cargo 改工作区 `Cargo.toml` 后执行 `cargo update -p 包`），并把结果记进任务 6 的说明文档。

结果与写计划时相同时，应用上面 `web/package.json` 的 diff 后更新锁文件：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm install
```

- [ ] **Step 4: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

有 Cargo 依赖升级时再跑后端全量检查（fmt、clippy、`DBLENS_LANG=en` 下的全部测试）。

Expected: tsc 无输出；20 个测试文件、89 个测试通过；打包成功。

- [ ] **Step 5: 提交**

```bash
git add scripts/check-deps.mjs web/package.json web/pnpm-lock.yaml
git commit -m "chore: 依赖复查脚本；i18next、react-i18next 升级到发布满 7 天的最新正式版" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Oracle / 达梦的 PL/SQL 解析器（含技术验证结论）

技术验证已在写计划时完成，全部达标（数据见任务 6 的说明文档）：gzip 1.42 MB（≤ 3 MB），1025 行脚本解析 94–147 ms（≤ 300 ms，首次 1.2 s），补全样例命中 19/20（≥ 80%）；达梦测试夹具 51 条中误报从 25 条降到 2 条（打补丁后）。因此 Oracle、达梦走完整流程。

本任务：保存上游语法；生成脚本打达梦补丁并用 `antlr-ng` 生成 TypeScript（不需要 Java，约 5 秒，生成结果不提交，`dev` / `build` / `test` / `typecheck` 前自动生成）；实现 PL/SQL 的语言服务：按 Oracle 规则分句（与后端 split.rs 一致）、逐句解析并缓存、antlr4-c3 分析补全、按词扫描表和别名。

**Files:**
- Modify: `.gitignore`
- Create: `web/grammars/plsql/README.md`
- Modify: `web/package.json`
- Create: `web/scripts/gen-plsql.mjs`
- Create: `web/src/features/sql/plsql/parser.test.ts`
- Create: `web/src/features/sql/plsql/parser.ts`
- Create: `web/src/features/sql/types.ts`
- Create（下载，见下）：`web/grammars/plsql/PlSqlLexer.g4`、`web/grammars/plsql/PlSqlLexerBase.ts`、`web/grammars/plsql/PlSqlParser.g4`、`web/grammars/plsql/PlSqlParserBase.ts`
- 由命令更新：`web/pnpm-lock.yaml`

**Interfaces:**
- Produces：
  - `web/src/features/sql/types.ts`：`Dialect = 'mysql' | 'postgres' | 'plsql'`、`ParserLocale`、`ParseIssue`（行列从 1 开始，结束列不含；`message?`、`incomplete?`、`near?`）、`Want`、`TableRef`、`Suggest { keywords, want, qualifier, tables }`、`SqlParser { validate(sql, locale), suggest(sql, offset) }`；
  - `web/src/features/sql/plsql/parser.ts`：`plsql: SqlParser`，另导出 `parseText`、`split`、`caretTokenIndex`、`scanTables`；
  - `web/scripts/gen-plsql.mjs`：导出 `PATCHES`、`patchGrammar`；`pnpm gen` 生成到 `web/src/features/sql/plsql/gen/`（`.gitignore` 忽略）。

- [ ] **Step 1: 取上游 PL/SQL 语法**

grammars-v4 没有发布版，固定到提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17，`[PlSql] Fixed valid aliases`，发布满 7 天）。四个文件按原样保存，不做修改：

```bash
mkdir -p web/grammars/plsql
R=https://raw.githubusercontent.com/antlr/grammars-v4/afbe4ed460d091afd7158b5cbd4ef63e64523e21/sql/plsql
curl -sSfL -o web/grammars/plsql/PlSqlLexer.g4 $R/PlSqlLexer.g4
curl -sSfL -o web/grammars/plsql/PlSqlParser.g4 $R/PlSqlParser.g4
curl -sSfL -o web/grammars/plsql/PlSqlLexerBase.ts $R/Antlr4ng/PlSqlLexerBase.ts
curl -sSfL -o web/grammars/plsql/PlSqlParserBase.ts $R/Antlr4ng/PlSqlParserBase.ts
wc -c web/grammars/plsql/*
```

Expected：`PlSqlLexer.g4` 119205 字节，`PlSqlParser.g4` 242620，`PlSqlLexerBase.ts` 684，`PlSqlParserBase.ts` 2780（均为 LF 换行）。

- [ ] **Step 2: 新建 `web/src/features/sql/plsql/parser.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { plsql } from './parser'

/** `|` 表示光标。 */
function at(text: string) {
  const offset = text.indexOf('|')
  return plsql.suggest(text.replace('|', ''), offset)
}

describe('PL/SQL 语法检查', () => {
  it('正确的 Oracle 语句没有问题', () => {
    const sql = `SELECT e.id, d.name FROM hr.emp e JOIN hr.dept d ON d.id = e.dept_id WHERE e.salary > 100 FETCH FIRST 10 ROWS ONLY;
MERGE INTO t USING (SELECT id FROM s) x ON (t.id = x.id) WHEN MATCHED THEN UPDATE SET t.v = 1;
CREATE OR REPLACE PACKAGE BODY p AS
  PROCEDURE x IS
  BEGIN
    FOR r IN (SELECT id FROM t) LOOP
      dbms_output.put_line(r.id);
    END LOOP;
  END x;
END p;
/
SELECT 1 FROM dual`
    expect(plsql.validate(sql, 'zh_CN')).toEqual([])
  })

  // 每条补丁（web/scripts/gen-plsql.mjs 的 PATCHES）对应一条语句
  it.each([
    'DROP FUNCTION IF EXISTS f',
    'DROP PACKAGE IF EXISTS p',
    'DROP PROCEDURE IF EXISTS p',
    'DROP TRIGGER IF EXISTS t',
    'DROP SEQUENCE IF EXISTS s',
    'DROP MATERIALIZED VIEW IF EXISTS mv',
    'DROP TABLE IF EXISTS t',
    'DROP VIEW IF EXISTS v',
    'DROP SYNONYM IF EXISTS s',
    'DROP TABLE t CASCADE',
    'CREATE TABLE t (id INT IDENTITY(1,1) NOT NULL, name VARCHAR(50 CHAR))',
    "COMMENT ON VIEW v IS '视图'",
    'CREATE TABLE t (a BIGINT, b TINYINT, c BYTE, d TEXT, e TIME)',
    'CREATE TABLE t (a VARBINARY(100), b NVARCHAR(20))',
    "CREATE TABLE t (id INT, d DATE) PARTITION BY RANGE (d) (PARTITION p1 VALUES LESS THAN (DATE '2024-01-01'))",
  ])('达梦语法补丁：%s', (sql) => {
    expect(plsql.validate(sql, 'zh_CN')).toEqual([])
  })

  it('标出出错的词；语句不完整时标出最后一个词', () => {
    expect(plsql.validate('SELECT * FORM t', 'zh_CN')).toEqual([
      { startLine: 1, startColumn: 10, endLine: 1, endColumn: 14, near: 'FORM' },
    ])
    expect(plsql.validate('SELECT * FROM t WHERE a =', 'zh_CN')).toEqual([
      { startLine: 1, startColumn: 25, endLine: 1, endColumn: 26, incomplete: true },
    ])
  })

  it('逐句检查，每句只报第一处；位置换算成全文中的行列', () => {
    const issues = plsql.validate('SELECT * FORM t WHERE WHERE;\nSELECT 1 FROM dual;\n  SELEC 2 FROM dual', 'zh_CN')
    expect(issues).toEqual([
      { startLine: 1, startColumn: 10, endLine: 1, endColumn: 14, near: 'FORM' },
      // SELEC 被当作过程名，报在后面的 2 上
      { startLine: 3, startColumn: 9, endLine: 3, endColumn: 10, near: '2' },
    ])
  })

  it('PL/SQL 块中的分号不分句，块以单独一行的 / 结束', () => {
    const sql = 'BEGIN\n  NULL;\n  NULL;\nEND;\n/\nSELECT 1 FROM dual;\nSELECT x FORM dual'
    expect(plsql.validate(sql, 'zh_CN').map((i) => i.startLine)).toEqual([7])
  })

  it('字符串没有结束是词法错误', () => {
    const issues = plsql.validate("SELECT 'abc FROM dual", 'zh_CN')
    expect(issues[0]).toMatchObject({ startLine: 1, startColumn: 8 })
  })
})

describe('PL/SQL 补全分析', () => {
  it.each([
    ['SELECT * FROM |', ['table', 'view']],
    ['SELECT * FROM t1, |', ['table', 'view']],
    ['UPDATE |', ['table', 'view']],
    ['DELETE FROM |', ['table', 'view']],
    ['MERGE INTO |', ['table', 'view']],
    ['SELECT * FROM a; SELECT * FROM |', ['table', 'view']],
    ['SELECT x FROM t WHERE x IN (SELECT y FROM |', ['table', 'view']],
    ['BEGIN UPDATE |', ['table', 'view']],
  ])('%s 需要表', (text, want) => {
    expect(at(text).want).toEqual(expect.arrayContaining(want))
  })

  it.each(['SELECT * FROM emp e WHERE |', 'SELECT a FROM t ORDER BY |', 'SELECT |', 'SELECT * FROM emp e JOIN dept d ON |'])(
    '%s 需要列和函数',
    (text) => {
      expect(at(text).want).toEqual(expect.arrayContaining(['column', 'function']))
    },
  )

  it('UPDATE … SET 需要列', () => {
    expect(at('UPDATE emp SET |').want).toContain('column')
  })

  it('限定名与语句中的表和别名', () => {
    const s = at('SELECT e.| FROM hr.employees e JOIN dept d ON d.id = e.dept_id')
    expect(s.qualifier).toEqual(['e'])
    expect(s.tables).toEqual([
      { schema: 'hr', name: 'employees', alias: 'e' },
      { name: 'dept', alias: 'd' },
    ])
    expect(at('SELECT * FROM hr.|').qualifier).toEqual(['hr'])
    expect(at('SELECT * FROM emp WHERE emp.na|').qualifier).toEqual(['emp'])
  })

  it('子查询中也能看到外层的表；只看光标所在的语句', () => {
    const s = at('SELECT 1 FROM other o;\nSELECT * FROM emp e WHERE EXISTS (SELECT 1 FROM dept d WHERE d.id = |)')
    expect(s.tables.map((t) => t.alias)).toEqual(['e', 'd'])
  })

  it('双引号名称去掉引号', () => {
    expect(at('SELECT * FROM "HR"."Emp Tab" x WHERE x.|').tables).toEqual([{ schema: 'HR', name: 'Emp Tab', alias: 'x' }])
  })

  it('关键字只给语法允许的', () => {
    const kw = at('SELECT * FROM emp |').keywords
    expect(kw).toEqual(expect.arrayContaining(['WHERE', 'JOIN', 'ORDER']))
    expect(kw).not.toContain('SELECT')
  })
})
```

- [ ] **Step 3: 修改 `.gitignore`**

```diff
--- a/.gitignore
+++ b/.gitignore
@@ -7,6 +7,8 @@ node_modules/
 
 # 前端构建产物
 /web/dist/
+# 生成的 PL/SQL 解析器（web/scripts/gen-plsql.mjs）
+/web/src/features/sql/plsql/gen/
 # tauri-build 生成的权限描述
 /crates/dblens-desktop/gen/
 # tauri-build 按 build.rs 中的命令列表自动生成的权限定义
```

- [ ] **Step 4: 新建 `web/grammars/plsql/README.md`**

```markdown
# PL/SQL 语法（Oracle、达梦）

来源：[antlr/grammars-v4](https://github.com/antlr/grammars-v4) 的 `sql/plsql`，提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17）。许可证 Apache-2.0（见 `.g4` 文件头）。

| 文件 | 上游路径 |
|---|---|
| `PlSqlLexer.g4`、`PlSqlParser.g4` | `sql/plsql/` |
| `PlSqlLexerBase.ts`、`PlSqlParserBase.ts` | `sql/plsql/Antlr4ng/` |

这里的文件与上游一致，不直接修改。`web/scripts/gen-plsql.mjs` 生成解析器时：
1. 按上游 `Antlr4ng/transformGrammar.py` 的做法，在语法里插入基类的 import；
2. 打上该脚本 `PATCHES` 中的补丁（达梦语法和上游缺陷，每条都有说明和测试）；
3. 用 `antlr-ng` 生成 TypeScript，输出到 `src/features/sql/plsql/gen/`（不提交，`pnpm dev` / `build` / `test` / `typecheck` 前自动生成，输入不变时跳过）。

更新上游版本：按依赖原则（只取发布满 7 天的提交），替换这四个文件并更新上面的提交号；补丁对不上时脚本会报错，按新语法调整补丁。
```

- [ ] **Step 5: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -5,10 +5,11 @@
   "type": "module",
   "packageManager": "pnpm@11.8.0",
   "scripts": {
-    "dev": "vite",
-    "build": "tsc --noEmit && vite build",
-    "test": "vitest run",
-    "typecheck": "tsc --noEmit",
+    "gen": "node scripts/gen-plsql.mjs",
+    "dev": "node scripts/gen-plsql.mjs && vite",
+    "build": "node scripts/gen-plsql.mjs && tsc --noEmit && vite build",
+    "test": "node scripts/gen-plsql.mjs && vitest run",
+    "typecheck": "node scripts/gen-plsql.mjs && tsc --noEmit",
     "tauri": "cd .. && tauri",
     "package": "cd .. && tauri build --config crates/dblens-desktop/tauri.bundle.json"
   },
@@ -20,6 +21,8 @@
     "@tanstack/react-query": "5.104.5",
     "@tauri-apps/api": "2.12.4",
     "antd": "6.6.7",
+    "antlr4-c3": "3.4.4",
+    "antlr4ng": "3.0.16",
     "i18next": "26.5.2",
     "monaco-editor": "0.57.0",
     "react": "19.3.1",
@@ -38,6 +41,7 @@
     "@types/react": "19.3.0",
     "@types/react-dom": "19.3.0",
     "@vitejs/plugin-react": "6.1.2",
+    "antlr-ng": "1.0.10",
     "jsdom": "30.1.3",
     "typescript": "7.0.2",
     "vite": "8.3.5",
```

- [ ] **Step 6: 新建 `web/scripts/gen-plsql.mjs`**

```js
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
```

- [ ] **Step 7: 新建 `web/src/features/sql/plsql/parser.ts`**

```ts
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

import type { ParseIssue, SqlParser, Suggest, TableRef, Want } from '../types'
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
}
```

- [ ] **Step 8: 新建 `web/src/features/sql/types.ts`**

```ts
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

/** 语言服务：每种 Dialect 一个实现，在 worker 里运行（测试中直接调用）。 */
export interface SqlParser {
  validate(sql: string, locale: ParserLocale): ParseIssue[]
  /** offset：光标在 sql 中的 UTF-16 偏移。 */
  suggest(sql: string, offset: number): Suggest
}
```

- [ ] **Step 9: 依赖**

按上面 `web/package.json` 的 diff 修改后（scripts 加生成步骤；依赖 `antlr4ng` 3.0.16、`antlr4-c3` 3.4.4，开发依赖 `antlr-ng` 1.0.10），执行：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm install; pnpm gen
```

Expected：最后一行 `已生成 PL/SQL 解析器（约 5 秒）`；再执行一次 `pnpm gen` 没有输出（输入未变，跳过）。

- [ ] **Step 10: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

另外确认打包结果中 PL/SQL 解析器单独成块：`dist/assets/` 下没有超过 7 MB 的新文件（解析器要到任务 3 接入 worker 后才进入打包）。

Expected: tsc 无输出；21 个测试文件、126 个测试通过（新增 `plsql/parser.test.ts` 37 个）；打包成功。

- [ ] **Step 11: 提交**

```bash
git add .gitignore web/grammars/plsql/PlSqlLexer.g4 web/grammars/plsql/PlSqlLexerBase.ts web/grammars/plsql/PlSqlParser.g4 web/grammars/plsql/PlSqlParserBase.ts web/grammars/plsql/README.md web/package.json web/pnpm-lock.yaml web/scripts/gen-plsql.mjs web/src/features/sql/plsql/parser.test.ts web/src/features/sql/plsql/parser.ts web/src/features/sql/types.ts
git commit -m "feat(web): Oracle/达梦 PL/SQL 解析器：grammars-v4 语法加达梦补丁，antlr-ng 生成，逐句检查与补全分析" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: SQL 语言 worker 与 MySQL / PG 解析器

dt-sql-parser 的 MySQL、PostgreSQL 解析器适配成 `SqlParser`；worker 按方言懒加载解析器（MySQL、PG、PL/SQL 各一块）；主线程客户端负责请求序号、首次加载、2 秒超时、加载失败和 worker 出错。

**Files:**
- Modify: `web/package.json`
- Create: `web/src/features/sql/client.test.ts`
- Create: `web/src/features/sql/client.ts`
- Create: `web/src/features/sql/dialects.test.ts`
- Create: `web/src/features/sql/dialects.ts`
- Create: `web/src/features/sql/dtsql.test.ts`
- Create: `web/src/features/sql/dtsql.ts`
- Create: `web/src/features/sql/mysql.ts`
- Create: `web/src/features/sql/parsers.ts`
- Create: `web/src/features/sql/postgres.ts`
- Create: `web/src/features/sql/protocol.ts`
- Create: `web/src/features/sql/worker.ts`
- 由命令更新：`web/pnpm-lock.yaml`

**Interfaces:**
- Consumes：任务 2 的 `types.ts`、`plsql`。
- Produces：
  - `dialects.ts`：`sqlDialect(driver, variant?) -> { dialect, strict, grammar }`（标准档：mysql、mariadb、postgres、oracle 变体；其余和未知变体为兼容档）；
  - `mysql.ts` 的 `mysql`、`postgres.ts` 的 `postgres`（`SqlParser`）；`dtsql.ts` 的 `adapt`、`caretAt(sql, offset)`；
  - `parsers.ts`：`loadParser(dialect): Promise<SqlParser>`；`worker.ts`；`protocol.ts`：`Request`、`RequestBody`、`Response`；
  - `client.ts`：`SqlClient(create: () => WorkerLike)`，`validate(dialect, sql, locale)`、`suggest(dialect, sql, offset)` 返回 `Outcome<T> = { ok: true, value } | { ok: false, reason: 'timeout' | 'unavailable' }`，`unavailable(dialect)`；`TIMEOUT_MS = 2000`；`WorkerLike`。

- [ ] **Step 1: 新建 `web/src/features/sql/client.test.ts`**

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { SqlClient, TIMEOUT_MS, type WorkerLike } from './client'
import type { Request, Response } from './protocol'

/** 假 worker：记下收到的请求，由测试决定怎么回复。 */
class FakeWorker implements WorkerLike {
  sent: Request[] = []
  onmessage: ((e: MessageEvent<Response>) => void) | null = null
  onerror: ((e: unknown) => void) | null = null
  postMessage(msg: Request) {
    this.sent.push(msg)
    if (msg.op === 'load') this.reply({ id: msg.id, ok: true, result: null })
  }
  reply(r: Response) {
    queueMicrotask(() => this.onmessage?.({ data: r } as MessageEvent<Response>))
  }
}

describe('SqlClient', () => {
  let w: FakeWorker
  let client: SqlClient
  beforeEach(() => {
    vi.useFakeTimers()
    vi.spyOn(console, 'warn').mockImplementation(() => {})
    w = new FakeWorker()
    client = new SqlClient(() => w)
  })
  afterEach(() => {
    vi.useRealTimers()
    vi.restoreAllMocks()
  })

  it('先加载解析器，再按序号返回结果', async () => {
    const p = client.validate('mysql', 'SELECT', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    expect(w.sent.map((r) => r.op)).toEqual(['load', 'validate'])
    const issues = [{ startLine: 1, startColumn: 1, endLine: 1, endColumn: 7, incomplete: true }]
    w.reply({ id: w.sent[1].id, ok: true, result: issues })
    await expect(p).resolves.toEqual({ ok: true, value: issues })
    // 同一方言不再加载
    void client.validate('mysql', 'SELECT 1', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(3))
    expect(w.sent[2].op).toBe('validate')
  })

  it('超过 2 秒放弃结果', async () => {
    const p = client.suggest('postgres', 'SELECT ', 7)
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    vi.advanceTimersByTime(TIMEOUT_MS + 1)
    await expect(p).resolves.toEqual({ ok: false, reason: 'timeout' })
  })

  it('解析器加载失败后该方言不再使用', async () => {
    w.postMessage = (msg) => {
      w.sent.push(msg)
      w.reply({ id: msg.id, ok: false, error: 'chunk load failed' })
    }
    await expect(client.validate('plsql', 'x', 'zh_CN')).resolves.toEqual({ ok: false, reason: 'unavailable' })
    expect(client.unavailable('plsql')).toBe(true)
    expect(client.unavailable('mysql')).toBe(false)
    await client.validate('plsql', 'y', 'zh_CN')
    expect(w.sent).toHaveLength(1)
  })

  it('worker 出错后全部不可用，等待中的请求立即结束', async () => {
    const p = client.validate('mysql', 'SELECT', 'zh_CN')
    await vi.waitFor(() => expect(w.sent).toHaveLength(2))
    w.onerror?.(new Error('boom'))
    await expect(p).resolves.toEqual({ ok: false, reason: 'unavailable' })
    expect(client.unavailable('postgres')).toBe(true)
  })
})
```

- [ ] **Step 2: 新建 `web/src/features/sql/dialects.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { sqlDialect } from './dialects'

describe('sqlDialect', () => {
  it.each([
    ['mysql', 'mysql', 'mysql', true],
    ['mysql', 'mariadb', 'mysql', true],
    ['mysql', 'tidb', 'mysql', false],
    ['mysql', 'oceanbase', 'mysql', false],
    ['mysql', 'polardb', 'mysql', false],
    ['postgres', 'postgres', 'postgres', true],
    ['postgres', 'kingbase', 'postgres', false],
    ['postgres', 'opengauss', 'postgres', false],
    ['postgres', 'gaussdb', 'postgres', false],
    ['postgres', 'highgo', 'postgres', false],
    ['oracle', 'oracle', 'plsql', true],
    ['dameng', 'dameng', 'plsql', false],
  ] as const)('%s / %s → %s，标准档 %s', (driver, variant, dialect, strict) => {
    expect(sqlDialect(driver, variant)).toMatchObject({ dialect, strict })
  })

  it('变体未知时按驱动选解析器，按兼容档处理', () => {
    expect(sqlDialect('oracle', null)).toEqual({ dialect: 'plsql', strict: false, grammar: 'Oracle' })
    expect(sqlDialect('mysql')).toEqual({ dialect: 'mysql', strict: false, grammar: 'MySQL' })
  })
})
```

- [ ] **Step 3: 新建 `web/src/features/sql/dtsql.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { caretAt } from './dtsql'
import { mysql } from './mysql'
import { postgres } from './postgres'
import type { SqlParser } from './types'

/** `|` 表示光标。 */
const at = (p: SqlParser, text: string) => p.suggest(text.replace('|', ''), text.indexOf('|'))

describe('dt-sql-parser 适配', () => {
  it('偏移换算成行列', () => {
    expect(caretAt('SELECT 1;\nSELECT ', 17)).toEqual({ lineNumber: 2, column: 8 })
  })

  it('语法错误按语言给出文字', () => {
    const [zh] = mysql.validate('SELECT * FORM t', 'zh_CN')
    expect(zh).toMatchObject({ startLine: 1, startColumn: 10, endLine: 1, endColumn: 14 })
    expect(zh.message).toContain('FORM')
    expect(zh.message).toMatch(/[一-鿿]/)
    const [en] = postgres.validate('SELECT * FROM t WHERE', 'en_US')
    expect(en.message).toBe('Statement is incomplete')
  })

  it('正确的语句没有问题', () => {
    expect(mysql.validate('SELECT e.id FROM emp e JOIN dept d ON d.id = e.dept_id LIMIT 10;\nUPDATE emp SET a = 1', 'zh_CN')).toEqual([])
    expect(postgres.validate('INSERT INTO t (a) VALUES (1) RETURNING id', 'zh_CN')).toEqual([])
  })

  it('FROM 后需要表', () => {
    expect(at(mysql, 'SELECT * FROM |').want).toEqual(expect.arrayContaining(['table', 'view']))
    expect(at(postgres, 'SELECT * FROM |').want).toEqual(expect.arrayContaining(['table', 'view']))
  })

  it('schema 限定', () => {
    const s = at(mysql, 'SELECT * FROM db1.|')
    expect(s.want).toContain('table')
    expect(s.qualifier).toEqual(['db1'])
  })

  it('别名限定的列，语句中的表和别名', () => {
    const s = at(mysql, 'SELECT a.| FROM emp a JOIN dept d ON a.did = d.id')
    expect(s.want).toContain('column')
    expect(s.qualifier).toEqual(['a'])
    expect(s.tables).toEqual([
      { name: 'emp', alias: 'a' },
      { name: 'dept', alias: 'd' },
    ])
    expect(at(postgres, 'SELECT e.| FROM hr.emp e').tables).toEqual([{ schema: 'hr', name: 'emp', alias: 'e' }])
  })

  it('正在输入的词不算限定名', () => {
    expect(at(mysql, 'SELECT * FROM emp e WHERE e.na|').qualifier).toEqual(['e'])
    expect(at(mysql, 'SELECT na| FROM emp').qualifier).toEqual([])
  })

  it('只看光标所在的语句', () => {
    const s = at(mysql, 'SELECT * FROM a1 x;\nSELECT * FROM emp WHERE id = 1 AND |;\nSELECT 2')
    expect(s.want).toContain('column')
  })

  it('反引号名称去掉引号', () => {
    expect(at(mysql, 'SELECT t.| FROM `my db`.`t 1` t').tables).toEqual([{ schema: 'my db', name: 't 1', alias: 't' }])
  })
})
```

- [ ] **Step 4: 修改 `web/package.json`**

```diff
--- a/web/package.json
+++ b/web/package.json
@@ -23,6 +23,7 @@
     "antd": "6.6.7",
     "antlr4-c3": "3.4.4",
     "antlr4ng": "3.0.16",
+    "dt-sql-parser": "4.5.1",
     "i18next": "26.5.2",
     "monaco-editor": "0.57.0",
     "react": "19.3.1",
```

- [ ] **Step 5: 新建 `web/src/features/sql/client.ts`**

```ts
// 主线程一侧的 SQL 语言服务：把请求发给 worker，按序号对应结果（M1b2 设计文档第 4、7 节）。
// - 所有方言共用一个 worker，解析器在 worker 中按需加载；
// - 某个方言第一次使用时先加载解析器（不计超时），之后每次请求超过 2 秒就放弃结果；
// - worker 出错或某个方言加载失败后，该方言在本次运行中不再使用解析器（调用方退回轻量补全、不检查语法）。

import type { Request, RequestBody, Response } from './protocol'
import type { Dialect, ParseIssue, ParserLocale, Suggest } from './types'

export const TIMEOUT_MS = 2000
/** 加载解析器（下载 + 预热）最多等这么久。 */
const LOAD_TIMEOUT_MS = 60_000

/** worker 的最小接口，测试中用假的实现。 */
export interface WorkerLike {
  postMessage(msg: Request): void
  onmessage: ((e: MessageEvent<Response>) => void) | null
  onerror: ((e: unknown) => void) | null
}

/** 加载失败的标记。 */
const FAILED = Promise.resolve(false)

export type Outcome<T> = { ok: true; value: T } | { ok: false; reason: 'timeout' | 'unavailable' }

export class SqlClient {
  private worker: WorkerLike | null = null
  private seq = 0
  private pending = new Map<number, (r: Response | null) => void>()
  private loads = new Map<Dialect, Promise<boolean>>()
  private broken = false

  constructor(private readonly create: () => WorkerLike) {}

  /** 该方言的解析器是否不可用（加载失败或 worker 出错）。 */
  unavailable(d: Dialect): boolean {
    return this.broken || this.loads.get(d) === FAILED
  }

  validate(dialect: Dialect, sql: string, locale: ParserLocale): Promise<Outcome<ParseIssue[]>> {
    return this.call(dialect, { op: 'validate', dialect, sql, locale })
  }

  suggest(dialect: Dialect, sql: string, offset: number): Promise<Outcome<Suggest>> {
    return this.call(dialect, { op: 'suggest', dialect, sql, offset })
  }

  private async call<T>(dialect: Dialect, req: RequestBody): Promise<Outcome<T>> {
    if (!(await this.load(dialect))) return { ok: false, reason: 'unavailable' }
    const resp = await this.send(req, TIMEOUT_MS)
    if (resp === null) return { ok: false, reason: this.broken ? 'unavailable' : 'timeout' }
    if (!resp.ok) return { ok: false, reason: 'unavailable' }
    return { ok: true, value: resp.result as T }
  }

  private load(d: Dialect): Promise<boolean> {
    let p = this.loads.get(d)
    if (!p) {
      p = this.send({ op: 'load', dialect: d }, LOAD_TIMEOUT_MS).then((r) => {
        if (r?.ok) return true
        console.warn(`SQL 解析器（${d}）不可用`, r && !r.ok ? r.error : '超时')
        this.loads.set(d, FAILED)
        return false
      })
      this.loads.set(d, p)
    }
    return p
  }

  /** 发送请求；超时或 worker 出错时得到 null。 */
  private send(req: RequestBody, timeoutMs: number): Promise<Response | null> {
    if (this.broken) return Promise.resolve(null)
    const w = this.ensureWorker()
    const id = ++this.seq
    return new Promise((resolve) => {
      const timer = setTimeout(() => done(null), timeoutMs)
      const done = (r: Response | null) => {
        clearTimeout(timer)
        this.pending.delete(id)
        resolve(r)
      }
      this.pending.set(id, done)
      w.postMessage({ ...req, id } as Request)
    })
  }

  private ensureWorker(): WorkerLike {
    if (!this.worker) {
      const w = this.create()
      w.onmessage = (e) => this.pending.get(e.data.id)?.(e.data)
      w.onerror = (e) => {
        console.warn('SQL 语言 worker 出错', e)
        this.broken = true
        for (const done of [...this.pending.values()]) done(null)
      }
      this.worker = w
    }
    return this.worker
  }
}
```

- [ ] **Step 6: 新建 `web/src/features/sql/dialects.ts`**

```ts
// 驱动 / 变体 → 解析器与档位（M1b2 设计文档 3.2 节）。
// 标准库的语法错误标红；兼容库（在 MySQL / PG / Oracle 语法上有扩展）只标黄"可能有误"，避免误报显得刺眼。
// 变体未知（还没连接过）时按驱动取解析器，按兼容档处理。

import type { DriverId } from '../../shared/api'
import type { Dialect } from './types'

export interface SqlDialect {
  dialect: Dialect
  /** 标准档：语法错误按错误（红色）标出。 */
  strict: boolean
  /** 检查所用语法的名称，兼容档的提示中说明。 */
  grammar: 'MySQL' | 'PostgreSQL' | 'Oracle'
}

const STRICT = new Set(['mysql', 'mariadb', 'postgres', 'oracle'])

export function sqlDialect(driver: DriverId, variant?: string | null): SqlDialect {
  const strict = !!variant && STRICT.has(variant)
  switch (driver) {
    case 'mysql':
      return { dialect: 'mysql', strict, grammar: 'MySQL' }
    case 'postgres':
      return { dialect: 'postgres', strict, grammar: 'PostgreSQL' }
    case 'oracle':
    case 'dameng':
      return { dialect: 'plsql', strict, grammar: 'Oracle' }
  }
}
```

- [ ] **Step 7: 新建 `web/src/features/sql/dtsql.ts`**

```ts
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
```

- [ ] **Step 8: 新建 `web/src/features/sql/mysql.ts`**

```ts
// MySQL 系（MySQL、MariaDB、TiDB、OceanBase、PolarDB）的语言服务，单独成块懒加载。

import { MySQL } from 'dt-sql-parser'

import { adapt } from './dtsql'

export const mysql = adapt(new MySQL())
```

- [ ] **Step 9: 新建 `web/src/features/sql/parsers.ts`**

```ts
// 按语法族懒加载解析器：每种是一个单独的块，用到时才下载（MySQL、PG 各约 0.35 MB，PL/SQL 约 1.4 MB，gzip 后）。

import type { Dialect, SqlParser } from './types'

const loaded = new Map<Dialect, Promise<SqlParser>>()

export function loadParser(d: Dialect): Promise<SqlParser> {
  let p = loaded.get(d)
  if (!p) {
    switch (d) {
      case 'mysql':
        p = import('./mysql').then((m) => m.mysql)
        break
      case 'postgres':
        p = import('./postgres').then((m) => m.postgres)
        break
      case 'plsql':
        p = import('./plsql/parser').then((m) => m.plsql)
        break
    }
    // 加载失败时不缓存，下次重试
    p.catch(() => loaded.delete(d))
    loaded.set(d, p)
  }
  return p
}
```

- [ ] **Step 10: 新建 `web/src/features/sql/postgres.ts`**

```ts
// PG 系（PostgreSQL、KingbaseES、openGauss、GaussDB、HighGo）的语言服务，单独成块懒加载。

import { PostgreSQL } from 'dt-sql-parser'

import { adapt } from './dtsql'

export const postgres = adapt(new PostgreSQL())
```

- [ ] **Step 11: 新建 `web/src/features/sql/protocol.ts`**

```ts
// 主线程与 SQL 语言 worker 之间的消息。

import type { Dialect, ParseIssue, ParserLocale, Suggest } from './types'

export type Request =
  | { id: number; op: 'load'; dialect: Dialect }
  | { id: number; op: 'validate'; dialect: Dialect; sql: string; locale: ParserLocale }
  | { id: number; op: 'suggest'; dialect: Dialect; sql: string; offset: number }

/** 去掉 id 的请求（按各个分支分别去掉）。 */
export type RequestBody = Request extends infer R ? (R extends Request ? Omit<R, 'id'> : never) : never

export type Response =
  | { id: number; ok: true; result: ParseIssue[] | Suggest | null }
  | { id: number; ok: false; error: string }
```

- [ ] **Step 12: 新建 `web/src/features/sql/worker.ts`**

```ts
// SQL 语言 worker（M1b2 设计文档第 4 节）：解析在这里进行，不阻塞界面。只做解析，不碰元数据。

import { loadParser } from './parsers'
import type { Request, Response } from './protocol'

async function handle(r: Request): Promise<Response> {
  try {
    const p = await loadParser(r.dialect)
    switch (r.op) {
      case 'load':
        // 预热：第一次解析要建立预测缓存，较慢
        p.validate('SELECT 1', 'en_US')
        return { id: r.id, ok: true, result: null }
      case 'validate':
        return { id: r.id, ok: true, result: p.validate(r.sql, r.locale) }
      case 'suggest':
        return { id: r.id, ok: true, result: p.suggest(r.sql, r.offset) }
    }
  } catch (e) {
    return { id: r.id, ok: false, error: e instanceof Error ? e.message : String(e) }
  }
}

// tsconfig 用的是 DOM 的类型，没有 worker 全局对象的类型
const scope = self as unknown as { onmessage: (e: MessageEvent<Request>) => void; postMessage(m: Response): void }
scope.onmessage = (e) => {
  void handle(e.data).then((resp) => scope.postMessage(resp))
}
```

- [ ] **Step 13: 依赖**

按上面 `web/package.json` 的 diff 加入 `dt-sql-parser` 4.5.1（2026-08-25 发布的正式版），然后：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm install
```

- [ ] **Step 14: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；24 个测试文件、152 个测试通过（新增 dtsql 9 个、dialects 13 个、client 4 个）；打包成功。

- [ ] **Step 15: 提交**

```bash
git add web/package.json web/pnpm-lock.yaml web/src/features/sql/client.test.ts web/src/features/sql/client.ts web/src/features/sql/dialects.test.ts web/src/features/sql/dialects.ts web/src/features/sql/dtsql.test.ts web/src/features/sql/dtsql.ts web/src/features/sql/mysql.ts web/src/features/sql/parsers.ts web/src/features/sql/postgres.ts web/src/features/sql/protocol.ts web/src/features/sql/worker.ts
git commit -m "feat(web): SQL 语言 worker：dt-sql-parser 适配 MySQL/PG，按方言懒加载解析器，主线程请求超时与失败处理" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 补全

补全接入 Monaco：worker 分析光标处该出现什么，主线程查元数据生成补全项（列按表中的顺序在前，然后是表、视图、同义词、序列、schema、函数、关键字）。元数据放在 react-query 缓存中，按需取、最多等 1.5 秒；对象树刷新、执行成功的 DDL 后失效。解析器不可用或超时时退回轻量补全。

另外两处调整：vite 的 worker 输出改为 ES 模块（否则 worker 中的动态 import 不拆块，所有解析器会打进一个 10 MB 的文件）；`WorkerLike.onerror` 的参数类型改为 `ErrorEvent`，与浏览器的 `Worker` 一致。

**Files:**
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/SqlEditor.tsx`
- Modify: `web/src/features/sql/client.test.ts`
- Modify: `web/src/features/sql/client.ts`
- Create: `web/src/features/sql/completion.test.ts`
- Create: `web/src/features/sql/completion.ts`
- Create: `web/src/features/sql/functions.ts`
- Create: `web/src/features/sql/metadata.test.ts`
- Create: `web/src/features/sql/metadata.ts`
- Create: `web/src/features/sql/provider.ts`
- Create: `web/src/features/sql/service.ts`
- Modify: `web/src/features/tree/ObjectTree.tsx`
- Modify: `web/vite.config.ts`

**Interfaces:**
- Consumes：任务 3 的 `SqlClient`、`sqlDialect`、`Suggest`。
- Produces：
  - `metadata.ts`：`MetaReader { schemas, objects, columns, functions }`、`metaReader(qc, api, profileId, driver, waitMs = 1500)`、`metaKey(profileId, schema?)`、`findName(items, name)`、`defaultSchema(driver, user, database?)`；
  - `completion.ts`：`completionItems({ suggest, dialect, schema, meta }) -> Item[]`、`quoteIdent(name, dialect)`、`lightSuggest(text, offset)`；`functions.ts`：`builtinFunctions(dialect)`；
  - `service.ts`：`sqlClient`；`provider.ts`：`EditorSqlContext { driver, variant, schema, meta }`、`bindModel(path, get)`、`contextOf(model)`、`registerSqlProviders()`；
  - `SqlEditor` 新增属性 `sql: EditorSqlContext | null`。

- [ ] **Step 1: 修改 `web/src/features/sql/client.test.ts`**

```diff
--- a/web/src/features/sql/client.test.ts
+++ b/web/src/features/sql/client.test.ts
@@ -7,7 +7,7 @@ import type { Request, Response } from './protocol'
 class FakeWorker implements WorkerLike {
   sent: Request[] = []
   onmessage: ((e: MessageEvent<Response>) => void) | null = null
-  onerror: ((e: unknown) => void) | null = null
+  onerror: ((e: ErrorEvent) => void) | null = null
   postMessage(msg: Request) {
     this.sent.push(msg)
     if (msg.op === 'load') this.reply({ id: msg.id, ok: true, result: null })
@@ -66,7 +66,7 @@ describe('SqlClient', () => {
   it('worker 出错后全部不可用，等待中的请求立即结束', async () => {
     const p = client.validate('mysql', 'SELECT', 'zh_CN')
     await vi.waitFor(() => expect(w.sent).toHaveLength(2))
-    w.onerror?.(new Error('boom'))
+    w.onerror?.({ message: 'boom' } as ErrorEvent)
     await expect(p).resolves.toEqual({ ok: false, reason: 'unavailable' })
     expect(client.unavailable('postgres')).toBe(true)
   })
```

- [ ] **Step 2: 新建 `web/src/features/sql/completion.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { completionItems, lightSuggest, quoteIdent } from './completion'
import type { MetaReader } from './metadata'
import type { Suggest } from './types'

const meta: MetaReader = {
  schemas: async () => ['HR', 'SALES'],
  objects: async (s) =>
    s === 'HR'
      ? [
          { name: 'EMP', kind: 'table', comment: '员工' },
          { name: 'DEPT', kind: 'table' },
          { name: 'V_EMP', kind: 'view' },
          { name: 'EMP_SEQ', kind: 'sequence' },
        ]
      : [{ name: 'ORDERS', kind: 'table' }],
  columns: async (s, t) =>
    s === 'HR' && t === 'EMP'
      ? [
          { name: 'ID', type: 'NUMBER' },
          { name: 'NAME', type: 'VARCHAR2(50)', comment: '姓名' },
        ]
      : s === 'HR' && t === 'DEPT'
        ? [
            { name: 'ID', type: 'NUMBER' },
            { name: 'TITLE', type: 'VARCHAR2(20)' },
          ]
        : [],
  functions: async (s) => (s === 'HR' ? ['F_BONUS'] : []),
}

const sug = (p: Partial<Suggest>): Suggest => ({ keywords: [], want: [], qualifier: [], tables: [], ...p })
const run = (p: Partial<Suggest>, schema: string | null = 'HR') => completionItems({ suggest: sug(p), dialect: 'plsql', schema, meta })
const labels = async (p: Partial<Suggest>, kind?: string) => (await run(p)).filter((i) => !kind || i.kind === kind).map((i) => i.label)

describe('completionItems', () => {
  it('需要表时给出当前 schema 的表、视图和所有 schema', async () => {
    const items = await run({ want: ['table', 'view'] })
    expect(items.map((i) => [i.kind, i.label])).toEqual([
      ['table', 'EMP'],
      ['table', 'DEPT'],
      ['view', 'V_EMP'],
      ['schema', 'HR'],
      ['schema', 'SALES'],
    ])
    expect(items[0].detail).toBe('员工')
  })

  it('schema 限定时给出该 schema 的对象', async () => {
    expect(await labels({ want: ['table'], qualifier: ['sales'] })).toEqual(['ORDERS'])
  })

  it('别名限定时只给该表的列，语句中的名称不分大小写', async () => {
    const items = await run({ want: ['column', 'function'], qualifier: ['e'], tables: [{ name: 'emp', alias: 'e' }, { name: 'dept', alias: 'd' }] })
    expect(items.map((i) => i.label)).toEqual(['ID', 'NAME'])
    expect(items[1].detail).toBe('VARCHAR2(50) · 姓名')
  })

  it('不限定时给出语句中各表的列（同名列合并说明）、函数和序列', async () => {
    const items = await run({
      want: ['column', 'function'],
      keywords: ['CASE'],
      tables: [{ schema: 'hr', name: 'emp', alias: 'e' }, { name: 'DEPT' }],
    })
    const cols = items.filter((i) => i.kind === 'column')
    expect(cols.map((i) => i.label)).toEqual(['ID', 'NAME', 'TITLE'])
    expect(cols[0].detail).toBe('NUMBER · e; NUMBER · DEPT')
    expect(items.some((i) => i.kind === 'function' && i.label === 'NVL')).toBe(true)
    expect(items.some((i) => i.kind === 'function' && i.label === 'F_BONUS')).toBe(true)
    expect(items.some((i) => i.kind === 'keyword' && i.label === 'CASE')).toBe(true)
  })

  it('schema.表. 给出列；语句中没写的当前 schema 表名也能给列', async () => {
    expect(await labels({ want: ['column'], qualifier: ['HR', 'DEPT'] })).toEqual(['ID', 'TITLE'])
    expect(await labels({ want: ['column'], qualifier: ['emp'] })).toEqual(['ID', 'NAME'])
  })

  it('排序：列（按表中的顺序）、表、schema、函数、关键字', async () => {
    const items = await run({ want: ['column', 'table', 'function'], keywords: ['WHERE'], tables: [{ name: 'EMP' }, { name: 'DEPT' }] })
    const sorted = [...items].sort((a, b) => (a.sortText < b.sortText ? -1 : 1))
    expect(sorted.map((i) => i.kind).filter((k, i, a) => a.indexOf(k) === i)).toEqual(['column', 'table', 'view', 'sequence', 'schema', 'function', 'keyword'])
    expect(sorted.filter((i) => i.kind === 'column').map((i) => i.label)).toEqual(['ID', 'NAME', 'TITLE'])
  })

  it('当前 schema 未知时不给表，仍给 schema', async () => {
    expect(await completionItems({ suggest: sug({ want: ['table'] }), dialect: 'plsql', schema: null, meta }).then((x) => x.map((i) => i.kind))).toEqual([
      'schema',
      'schema',
    ])
  })
})

describe('quoteIdent', () => {
  it.each([
    ['EMP', 'plsql', 'EMP'],
    ['Emp', 'plsql', '"Emp"'],
    ['emp', 'postgres', 'emp'],
    ['Emp', 'postgres', '"Emp"'],
    ['my table', 'mysql', '`my table`'],
    ['Emp', 'mysql', 'Emp'],
    ['数洞表', 'plsql', '"数洞表"'],
  ] as const)('%s（%s）→ %s', (name, d, out) => {
    expect(quoteIdent(name, d)).toBe(out)
  })
})

describe('lightSuggest', () => {
  it('取限定名和语句中的表', () => {
    const text = 'SELECT e. FROM hr.emp e JOIN dept AS d ON 1 = 1 WHERE x'
    const s = lightSuggest(text, 'SELECT e.'.length)
    expect(s.qualifier).toEqual(['e'])
    expect(s.tables).toEqual([{ schema: 'hr', name: 'emp', alias: 'e' }, { name: 'dept', alias: 'd' }])
    expect(lightSuggest('SELECT * FROM t WHERE', 21).tables).toEqual([{ name: 't' }])
  })
})
```

- [ ] **Step 3: 新建 `web/src/features/sql/metadata.test.ts`**

```ts
import { QueryClient } from '@tanstack/react-query'
import { describe, expect, it, vi } from 'vitest'

import { type MetaApi, defaultSchema, findName, metaKey, metaReader } from './metadata'

function fakeApi(): MetaApi & { calls: string[] } {
  const calls: string[] = []
  const log = <T>(name: string, v: T) => {
    calls.push(name)
    return Promise.resolve(v)
  }
  return {
    calls,
    schemas: () => log('schemas', [{ name: 'HR', is_system: false }]),
    objects: (_id, s, kind) => log(`objects ${s} ${kind}`, kind === 'tables' ? [{ name: 'EMP', comment: null }] : [{ name: 'V1', comment: '视图' }]),
    table: (_id, s, t) =>
      log(`table ${s}.${t}`, {
        name: t,
        comment: null,
        primary_key: null,
        columns: [{ name: 'ID', ordinal: 1, data_type: { native: 'NUMBER', logical: null }, nullable: false, default: null, comment: null }],
      }),
    synonyms: (_id, s) => log(`synonyms ${s}`, [{ name: 'S1', target_schema: null, target_name: 'X', db_link: null }]),
    sequences: (_id, s) => log(`sequences ${s}`, [{ name: 'Q1' }]),
    routines: (_id, s) =>
      log(`routines ${s}`, [
        { name: 'F1', kind: 'function' as const },
        { name: 'P1', kind: 'procedure' as const },
      ]),
  }
}

describe('metaReader', () => {
  it('取对象（Oracle、达梦另取同义词和序列）并缓存', async () => {
    const qc = new QueryClient()
    const api = fakeApi()
    const m = metaReader(qc, api, 'p1', 'oracle')
    const objs = await m.objects('HR')
    expect(objs.map((o) => `${o.kind}:${o.name}`)).toEqual(['table:EMP', 'view:V1', 'synonym:S1', 'sequence:Q1'])
    await m.objects('HR')
    expect(api.calls.filter((c) => c.startsWith('objects'))).toHaveLength(2)
    expect(await m.columns('HR', 'EMP')).toEqual([{ name: 'ID', type: 'NUMBER', comment: null }])
    expect(await m.functions('HR')).toEqual(['F1'])
    expect(await m.schemas()).toEqual(['HR'])
  })

  it('MySQL 不取同义词和序列', async () => {
    const api = fakeApi()
    await metaReader(new QueryClient(), api, 'p1', 'mysql').objects('db')
    expect(api.calls).toEqual(['objects db tables', 'objects db views'])
  })

  it('失效后重新取', async () => {
    const qc = new QueryClient()
    const api = fakeApi()
    const m = metaReader(qc, api, 'p1', 'mysql')
    await m.columns('db', 't')
    await qc.invalidateQueries({ queryKey: metaKey('p1', 'db') })
    await m.columns('db', 't')
    expect(api.calls).toEqual(['table db.t', 'table db.t'])
  })

  it('出错或太慢时先返回空', async () => {
    vi.useFakeTimers()
    try {
      const api = fakeApi()
      api.table = () => new Promise(() => {})
      api.schemas = () => Promise.reject(new Error('locked'))
      const m = metaReader(new QueryClient(), api, 'p1', 'mysql', 100)
      const cols = m.columns('db', 't')
      await vi.advanceTimersByTimeAsync(101)
      await expect(cols).resolves.toEqual([])
      await expect(m.schemas()).resolves.toEqual([])
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('findName / defaultSchema', () => {
  it('先找完全相同的，再不分大小写', () => {
    expect(findName(['emp', 'EMP'], 'EMP')).toBe('EMP')
    expect(findName([{ name: 'EMP' }], 'emp')).toEqual({ name: 'EMP' })
    expect(findName(['A'], 'b')).toBeUndefined()
  })

  it('当前 schema 的推断', () => {
    expect(defaultSchema('mysql', 'root', 'shop')).toBe('shop')
    expect(defaultSchema('mysql', 'root', null)).toBeNull()
    expect(defaultSchema('postgres', 'admin', 'db')).toBe('public')
    expect(defaultSchema('oracle', 'dblens_test')).toBe('DBLENS_TEST')
  })
})
```

- [ ] **Step 4: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -1,6 +1,7 @@
 // 一个查询标签页的内容：工具栏、提示条、编辑器和结果面板（可拖动分隔线，位置存进设置）、确认框。
 // 处理编辑器命令：执行当前语句（有选中时执行选中部分）、执行全部、停止、格式化、执行计划。
 
+import { useQueryClient } from '@tanstack/react-query'
 import { Alert, App, Button } from 'antd'
 import { useCallback, useEffect, useMemo, useRef } from 'react'
 import { useTranslation } from 'react-i18next'
@@ -10,6 +11,7 @@ import { useApi } from '../../app/context'
 import { type Profile, errorText } from '../../shared/api'
 import { layoutStorage } from '../../shared/settings'
 import ResultsPanel from '../results/ResultsPanel'
+import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
 import ConfirmDialog from './ConfirmDialog'
 import Toolbar from './Toolbar'
 import { onCommand } from './commands'
@@ -19,8 +21,12 @@ import { runnerFor, useTabRuntime } from './runner'
 import SqlEditor, { type MonacoEditor } from './SqlEditor'
 import { type QueryTab, useTabs } from './tabs'
 
+/** 会改变对象结构的语句（按首个关键字）。 */
+const DDL = new Set(['CREATE', 'ALTER', 'DROP', 'RENAME', 'TRUNCATE', 'COMMENT'])
+
 export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles: Profile[] }) {
   const api = useApi()
+  const qc = useQueryClient()
   const { t } = useTranslation()
   const { message } = App.useApp()
   const runner = runnerFor(api, tab.id)
@@ -85,6 +91,31 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
   }, [protocolError, message])
 
   const marks = useMemo(() => errorMarks(rt.exec, rt.source), [rt.exec, rt.source])
+
+  // 执行成功的语句中有 DDL：补全用的元数据（连同后端的元数据缓存）失效，新建的表马上能补全
+  const refreshed = useRef<unknown>(null)
+  useEffect(() => {
+    if (rt.running || !profile || refreshed.current === rt.exec) return
+    refreshed.current = rt.exec
+    if (!rt.exec.statements.some((s) => !s.error && DDL.has(s.keyword))) return
+    void api
+      .refresh(profile.id, null)
+      .catch(() => {})
+      .then(() => qc.invalidateQueries({ queryKey: metaKey(profile.id) }))
+  }, [rt.running, rt.exec, profile, api, qc])
+  const meta = useMemo(() => (profile ? metaReader(qc, api, profile.id, profile.driver) : null), [qc, api, profile?.id, profile?.driver])
+  const sql = useMemo(
+    () =>
+      profile && meta
+        ? {
+            driver: profile.driver,
+            variant: rt.session?.variant ?? profile.variant,
+            schema: rt.session?.schema ?? defaultSchema(profile.driver, profile.user, profile.database),
+            meta,
+          }
+        : null,
+    [profile, meta, rt.session?.variant, rt.session?.schema],
+  )
   const confirm = !rt.running && !rt.declined ? rt.exec.confirm : null
   const sub = rt.submission
 
@@ -111,6 +142,7 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
             language={profile ? editorLanguage(profile.driver) : 'sql'}
             defaultValue={tab.sql}
             marks={marks}
+            sql={sql}
             onChange={(v) => useTabs.getState().setSql(tab.id, v)}
             onReady={onReady}
           />
```

- [ ] **Step 5: 修改 `web/src/features/editor/SqlEditor.tsx`**

```diff
--- a/web/src/features/editor/SqlEditor.tsx
+++ b/web/src/features/editor/SqlEditor.tsx
@@ -1,5 +1,6 @@
 // SQL 编辑器：Monaco。每个标签页一个模型（以标签 id 为路径），切换标签时保留各自的撤销历史和光标。
 // 快捷键（M1 设计文档 1.3 节）注册为编辑器动作，焦点在编辑器里时由 Monaco 处理；错误标记来自执行结果。
+// 补全（M1b2）：模型绑定所在标签页的连接上下文，由 features/sql/provider.ts 提供补全项。
 
 import Editor, { type OnMount } from '@monaco-editor/react'
 import { useEffect, useRef } from 'react'
@@ -7,6 +8,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useSetting } from '../../shared/settings'
 import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
+import { type EditorSqlContext, bindModel, registerSqlProviders } from '../sql/provider'
 import { type EditorCommand, sendCommand } from './commands'
 import { useCursor } from './cursor'
 import type { ErrorMark } from './markers'
@@ -14,6 +16,8 @@ import { monaco } from './monaco'
 
 export type MonacoEditor = Parameters<OnMount>[0]
 
+registerSqlProviders()
+
 const { KeyMod, KeyCode } = monaco
 type CommandLabel = `editor.command.${'runCurrent' | 'runAll' | 'stop' | 'format' | 'explain'}`
 // label 是词条键，注册动作时按当前界面语言翻译
@@ -30,15 +34,20 @@ export interface SqlEditorProps {
   language: string
   defaultValue: string
   marks: ErrorMark[]
+  /** 补全用的上下文；连接已删除时为 null。 */
+  sql: EditorSqlContext | null
   onChange(value: string): void
   onReady(editor: MonacoEditor | null): void
 }
 
-export default function SqlEditor({ path, language, defaultValue, marks, onChange, onReady }: SqlEditorProps) {
+export default function SqlEditor({ path, language, defaultValue, marks, sql, onChange, onReady }: SqlEditorProps) {
   const { t } = useTranslation()
   const systemDark = useSystemDark()
   const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
   const editorRef = useRef<MonacoEditor | null>(null)
+  const sqlRef = useRef(sql)
+  sqlRef.current = sql
+  useEffect(() => bindModel(path, () => sqlRef.current), [path])
 
   const onMount: OnMount = (editor) => {
     editorRef.current = editor
@@ -103,6 +112,8 @@ export default function SqlEditor({ path, language, defaultValue, marks, onChang
         tabSize: 2,
         fixedOverflowWidgets: true,
         renderLineHighlight: 'line',
+        // 只用 SQL 补全，不补文中出现过的词
+        wordBasedSuggestions: 'off',
       }}
     />
   )
```

- [ ] **Step 6: 修改 `web/src/features/sql/client.ts`**

```diff
--- a/web/src/features/sql/client.ts
+++ b/web/src/features/sql/client.ts
@@ -14,7 +14,7 @@ const LOAD_TIMEOUT_MS = 60_000
 export interface WorkerLike {
   postMessage(msg: Request): void
   onmessage: ((e: MessageEvent<Response>) => void) | null
-  onerror: ((e: unknown) => void) | null
+  onerror: ((e: ErrorEvent) => void) | null
 }
 
 /** 加载失败的标记。 */
```

- [ ] **Step 7: 新建 `web/src/features/sql/completion.ts`**

```ts
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
```

- [ ] **Step 8: 新建 `web/src/features/sql/functions.ts`**

```ts
// 常用内置函数（补全用的精简列表，不求完整）。用户自己的函数从元数据取。

import type { Dialect } from './types'

const MYSQL = `ABS AVG CAST CEIL COALESCE CONCAT CONCAT_WS CONVERT COUNT CURDATE CURRENT_DATE CURRENT_TIMESTAMP DATE DATE_ADD
DATE_FORMAT DATE_SUB DATEDIFF DAY FLOOR FROM_UNIXTIME GREATEST GROUP_CONCAT HOUR IF IFNULL INSTR JSON_ARRAY JSON_EXTRACT
JSON_OBJECT LAST_INSERT_ID LEAST LEFT LENGTH LOCATE LOWER LPAD LTRIM MAX MIN MINUTE MOD MONTH NOW NULLIF REPLACE RIGHT ROUND
ROW_NUMBER RPAD RTRIM STR_TO_DATE SUBSTRING SUBSTRING_INDEX SUM SYSDATE TIMESTAMPDIFF TRIM TRUNCATE UNIX_TIMESTAMP UPPER UUID YEAR`

const POSTGRES = `ABS AGE ARRAY_AGG AVG CAST CEIL COALESCE CONCAT COUNT CURRENT_DATE CURRENT_TIMESTAMP DATE_PART DATE_TRUNC
DENSE_RANK EXTRACT FLOOR GENERATE_SERIES GREATEST JSON_AGG JSONB_AGG JSONB_BUILD_OBJECT LAG LEAD LEAST LEFT LENGTH LOWER LPAD
MAX MIN NOW NULLIF RANK REGEXP_REPLACE REPLACE RIGHT ROUND ROW_NUMBER RPAD SPLIT_PART STRING_AGG SUBSTRING SUM TO_CHAR TO_DATE
TO_NUMBER TO_TIMESTAMP TRIM UPPER`

// Oracle 与达梦共用
const PLSQL = `ABS ADD_MONTHS AVG CAST CEIL COALESCE CONCAT COUNT DECODE DENSE_RANK EXTRACT FLOOR GREATEST INSTR LAG
LAST_DAY LEAD LEAST LENGTH LISTAGG LOWER LPAD LTRIM MAX MIN MOD MONTHS_BETWEEN NVL NVL2 RANK REGEXP_LIKE REGEXP_REPLACE
REGEXP_SUBSTR REPLACE ROUND ROW_NUMBER RPAD RTRIM SUBSTR SUM SYS_GUID SYSDATE SYSTIMESTAMP TO_CHAR TO_DATE TO_NUMBER
TO_TIMESTAMP TRIM TRUNC UPPER`

const LISTS: Record<Dialect, string[]> = {
  mysql: MYSQL.split(/\s+/),
  postgres: POSTGRES.split(/\s+/),
  plsql: PLSQL.split(/\s+/),
}

export function builtinFunctions(d: Dialect): string[] {
  return LISTS[d]
}
```

- [ ] **Step 9: 新建 `web/src/features/sql/metadata.ts`**

```ts
// 补全用的元数据（M1b2 设计文档第 4 节）：schema、对象、表的列、用户函数。
// - 放在 react-query 缓存里，键以 ['meta', 连接] 开头；对象树刷新时一并失效；
// - 按需取：表的列第一次需要时才取；
// - 补全不长时间等待：超过 waitMs 先返回空，请求继续在后台完成，下次补全时就有了；
// - 取不到（出错、密码库锁定）时当作空，不弹错误。

import type { QueryClient } from '@tanstack/react-query'

import type { Api, DriverId } from '../../shared/api'

export interface MetaObject {
  name: string
  kind: 'table' | 'view' | 'synonym' | 'sequence'
  comment?: string | null
}

export interface MetaColumn {
  name: string
  type: string
  comment?: string | null
}

export interface MetaReader {
  schemas(): Promise<string[]>
  objects(schema: string): Promise<MetaObject[]>
  columns(schema: string, table: string): Promise<MetaColumn[]>
  functions(schema: string): Promise<string[]>
}

export type MetaApi = Pick<Api, 'schemas' | 'objects' | 'table' | 'synonyms' | 'sequences' | 'routines'>

/** 某个连接（或其中一个 schema）的补全元数据的查询键前缀，对象树刷新时用来失效。 */
export const metaKey = (profileId: string, schema?: string) => (schema === undefined ? ['meta', profileId] : ['meta', profileId, schema])

/** 等 p 最多 ms 毫秒；超时或出错时得到 fallback（p 继续在后台完成）。 */
function within<T>(p: Promise<T>, ms: number, fallback: T): Promise<T> {
  return new Promise((resolve) => {
    const timer = setTimeout(() => resolve(fallback), ms)
    p.then(
      (v) => {
        clearTimeout(timer)
        resolve(v)
      },
      () => {
        clearTimeout(timer)
        resolve(fallback)
      },
    )
  })
}

export function metaReader(qc: QueryClient, api: MetaApi, profileId: string, driver: DriverId, waitMs = 1500): MetaReader {
  const get = <T>(key: unknown[], fn: () => Promise<T>, fallback: T) =>
    within(qc.fetchQuery({ queryKey: key, queryFn: fn, staleTime: Infinity }), waitMs, fallback)
  const plsql = driver === 'oracle' || driver === 'dameng'
  return {
    schemas: () => get([...metaKey(profileId), '#schemas'], async () => (await api.schemas(profileId)).map((s) => s.name), []),
    objects: (schema) =>
      get(
        [...metaKey(profileId, schema), 'objects'],
        async () => {
          const [tables, views, synonyms, sequences] = await Promise.all([
            api.objects(profileId, schema, 'tables'),
            api.objects(profileId, schema, 'views'),
            plsql ? api.synonyms(profileId, schema) : Promise.resolve([]),
            plsql ? api.sequences(profileId, schema) : Promise.resolve([]),
          ])
          return [
            ...tables.map((o): MetaObject => ({ name: o.name, kind: 'table', comment: o.comment })),
            ...views.map((o): MetaObject => ({ name: o.name, kind: 'view', comment: o.comment })),
            ...synonyms.map((o): MetaObject => ({ name: o.name, kind: 'synonym' })),
            ...sequences.map((o): MetaObject => ({ name: o.name, kind: 'sequence' })),
          ]
        },
        [],
      ),
    columns: (schema, table) =>
      get(
        [...metaKey(profileId, schema), 'columns', table],
        async () =>
          ((await api.table(profileId, schema, table))?.columns ?? []).map((c) => ({
            name: c.name,
            type: c.data_type.native,
            comment: c.comment,
          })),
        [],
      ),
    functions: (schema) =>
      get(
        [...metaKey(profileId, schema), 'functions'],
        async () => (await api.routines(profileId, schema)).filter((r) => r.kind === 'function').map((r) => r.name),
        [],
      ),
  }
}

/** 按名称找：先找完全相同的，再不分大小写（语句里写的 emp 对应 Oracle 中的 EMP）。 */
export function findName<T extends { name: string } | string>(items: T[], name: string): T | undefined {
  const n = (x: T) => (typeof x === 'string' ? x : x.name)
  return items.find((x) => n(x) === name) ?? items.find((x) => n(x).toUpperCase() === name.toUpperCase())
}

/** 没有打开会话时的当前 schema：MySQL 系是连接配置里的库，PG 系是 public，Oracle 和达梦是用户名（大写）。 */
export function defaultSchema(driver: DriverId, user: string, database?: string | null): string | null {
  switch (driver) {
    case 'mysql':
      return database || null
    case 'postgres':
      return 'public'
    case 'oracle':
    case 'dameng':
      return user.toUpperCase()
  }
}
```

- [ ] **Step 10: 新建 `web/src/features/sql/provider.ts`**

```ts
// Monaco 的 SQL 补全（M1b2 设计文档第 4 节）。三种语言（mysql、pgsql、plsql）共用一个补全提供者；
// 每个编辑器模型绑定它所在标签页的上下文（连接、变体、当前 schema、元数据），提供者按模型找到上下文。

import type { DriverId } from '../../shared/api'
import { monaco } from '../editor/monaco'
import { type ItemKind, completionItems, lightSuggest } from './completion'
import { sqlDialect } from './dialects'
import type { MetaReader } from './metadata'
import { sqlClient } from './service'

export interface EditorSqlContext {
  driver: DriverId
  variant: string | null | undefined
  /** 当前 schema：会话的，没有会话时按连接配置推断。 */
  schema: string | null
  meta: MetaReader
}

const contexts = new Map<string, () => EditorSqlContext | null>()

/** 把编辑器模型（按 path）绑定到上下文；返回解绑函数。 */
export function bindModel(path: string, get: () => EditorSqlContext | null): () => void {
  const key = monaco.Uri.parse(path).toString()
  contexts.set(key, get)
  return () => {
    if (contexts.get(key) === get) contexts.delete(key)
  }
}

export function contextOf(model: monaco.editor.ITextModel): EditorSqlContext | null {
  return contexts.get(model.uri.toString())?.() ?? null
}

const { CompletionItemKind: K } = monaco.languages
const KIND: Record<ItemKind, monaco.languages.CompletionItemKind> = {
  keyword: K.Keyword,
  schema: K.Module,
  table: K.Struct,
  view: K.Interface,
  synonym: K.Reference,
  sequence: K.Value,
  column: K.Field,
  function: K.Function,
}

const provider: monaco.languages.CompletionItemProvider = {
  triggerCharacters: ['.'],
  async provideCompletionItems(model, position) {
    const ctx = contextOf(model)
    if (!ctx) return { suggestions: [] }
    const { dialect } = sqlDialect(ctx.driver, ctx.variant)
    const text = model.getValue()
    const offset = model.getOffsetAt(position)
    // 解析器不可用或超时：退回轻量补全
    const r = sqlClient.unavailable(dialect) ? null : await sqlClient.suggest(dialect, text, offset)
    const suggest = r?.ok ? r.value : lightSuggest(text, offset)
    const items = await completionItems({ suggest, dialect, schema: ctx.schema, meta: ctx.meta })
    const word = model.getWordUntilPosition(position)
    const range = new monaco.Range(position.lineNumber, word.startColumn, position.lineNumber, word.endColumn)
    return {
      suggestions: items.map((i) => ({
        label: i.label,
        kind: KIND[i.kind],
        detail: i.detail,
        insertText: i.insertText,
        sortText: i.sortText,
        range,
      })),
    }
  },
}

let registered = false

/** 注册补全提供者（重复调用无效果）。 */
export function registerSqlProviders() {
  if (registered) return
  registered = true
  for (const lang of ['mysql', 'pgsql', 'plsql']) monaco.languages.registerCompletionItemProvider(lang, provider)
}
```

- [ ] **Step 11: 新建 `web/src/features/sql/service.ts`**

```ts
// 全局唯一的 SQL 语言服务客户端。worker 在第一次使用时才创建。

import { SqlClient } from './client'
import SqlWorker from './worker?worker'

export const sqlClient = new SqlClient(() => new SqlWorker())
```

- [ ] **Step 12: 修改 `web/src/features/tree/ObjectTree.tsx`**

```diff
--- a/web/src/features/tree/ObjectTree.tsx
+++ b/web/src/features/tree/ObjectTree.tsx
@@ -28,6 +28,7 @@ import DbIcon from '../../shared/DbIcon'
 import { driverLabel } from '../connections/form'
 import { variantName } from '../editor/dialect'
 import { withVault } from '../connections/vault'
+import { metaKey } from '../sql/metadata'
 import {
   type CategoryKind,
   type NodeKey,
@@ -171,6 +172,8 @@ export default function ObjectTree() {
     const k: NodeKey = node.type === 'g' ? { type: 'c', profileId: node.profileId } : node
     const key = encodeKey(k)
     await api.refresh(k.profileId, 'schema' in k ? k.schema : null).catch(() => {})
+    // 补全用的元数据一并失效
+    void qc.invalidateQueries({ queryKey: metaKey(k.profileId, 'schema' in k ? k.schema : undefined) })
     const under = (x: string) => isWithin(decodeKey(x), k)
     setLoaded((m) => Object.fromEntries(Object.entries(m).filter(([x]) => x !== key && !under(x))))
     setExpanded((e) => e.filter((x) => !under(String(x))))
```

- [ ] **Step 13: 修改 `web/vite.config.ts`**

```diff
--- a/web/vite.config.ts
+++ b/web/vite.config.ts
@@ -18,6 +18,8 @@ export default defineConfig({
     outDir: 'dist',
     chunkSizeWarningLimit: 2048,
   },
+  // SQL 语言 worker 按方言懒加载解析器：需要 ES 模块格式才能拆块（默认的 iife 会把所有解析器打进一个文件）
+  worker: { format: 'es' },
   test: {
     setupFiles: ['./src/test-setup.ts'],
   },
```

- [ ] **Step 14: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；26 个测试文件、173 个测试通过（新增 completion 15 个、metadata 6 个）；打包成功，`dist/assets/` 中有 `parser-*.js`（约 6.7 MB）、`mysql-*.js`（约 1.8 MB）、`postgres-*.js`（约 1.7 MB）三个单独的块。

- [ ] **Step 15: 提交**

```bash
git add web/src/features/editor/QueryView.tsx web/src/features/editor/SqlEditor.tsx web/src/features/sql/client.test.ts web/src/features/sql/client.ts web/src/features/sql/completion.test.ts web/src/features/sql/completion.ts web/src/features/sql/functions.ts web/src/features/sql/metadata.test.ts web/src/features/sql/metadata.ts web/src/features/sql/provider.ts web/src/features/sql/service.ts web/src/features/tree/ObjectTree.tsx web/vite.config.ts
git commit -m "feat(web): SQL 补全：关键字、schema、表和视图、列（含别名）、函数，元数据按需取并随对象树刷新和 DDL 失效" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 编辑时的语法检查

编辑后 400 ms 检查整个脚本：标准库标红，兼容库标黄并说明按哪种语法检查；与执行出错的标记来源不同，互不覆盖。设置 `ui.editor.syntax-check` 可关（设置名只允许小写字母、数字和 `._-`）。脚本超过 1 MB 不检查；连续三次超时停止检查。状态栏显示问题数，点击跳到第一处。

**Files:**
- Modify: `web/src/app/SettingsDialog.tsx`
- Modify: `web/src/app/StatusBar.tsx`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/SqlEditor.tsx`
- Modify: `web/src/features/editor/commands.ts`
- Create: `web/src/features/sql/checker.ts`
- Create: `web/src/features/sql/diagnostics.test.ts`
- Create: `web/src/features/sql/diagnostics.ts`
- Modify: `web/src/features/sql/provider.ts`
- Create: `web/src/features/sql/syntax.ts`
- Modify: `web/src/locales/en/app.json`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/app.json`
- Modify: `web/src/locales/zh-CN/editor.json`

**Interfaces:**
- Consumes：任务 4 的 `bindModel`、`contextOf`、`sqlClient`。
- Produces：
  - `syntax.ts`：`SYNTAX_KEY = 'ui.editor.syntax-check'`、`syntaxEnabled(raw)`、`SyntaxState { status: 'ok' | 'stopped' | 'skipped' | 'off', count, first }`、`useSyntax`；
  - `diagnostics.ts`：`syntaxMarks(issues, dialect, name) -> SyntaxMark[]`；`checker.ts`：`attachChecker(editor) -> { recheck, dispose }`；
  - `provider.ts` 的绑定改为记录标签页 id：`pathOf(model)`；
  - 编辑器命令 `first-problem`；词条 `editor.syntax.*`、`app.status.syntax*`、`app.settings.syntaxCheck*`。

- [ ] **Step 1: 新建 `web/src/features/sql/diagnostics.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import { syntaxMarks } from './diagnostics'

const issue = { startLine: 2, startColumn: 5, endLine: 2, endColumn: 9 }

describe('syntaxMarks', () => {
  it('标准档：错误，文字用解析器给的', () => {
    const [m] = syntaxMarks([{ ...issue, message: "'FORM' 在此位置无效" }], { dialect: 'mysql', strict: true, grammar: 'MySQL' }, 'MySQL')
    expect(m).toEqual({
      severity: 'error',
      message: "'FORM' 在此位置无效",
      startLineNumber: 2,
      startColumn: 5,
      endLineNumber: 2,
      endColumn: 9,
    })
  })

  it('兼容档：警告，说明按哪种语法检查', () => {
    const [m] = syntaxMarks([{ ...issue, near: 'LIMIT' }], { dialect: 'plsql', strict: false, grammar: 'Oracle' }, '达梦')
    expect(m.severity).toBe('warning')
    expect(m.message).toBe('“LIMIT”附近有语法错误（按 Oracle 语法检查，达梦 的扩展语法可能被误报）')
  })

  it('没有文字时按不完整 / 出错的词生成；至少标一个字符', () => {
    const [m] = syntaxMarks([{ ...issue, endColumn: 5, incomplete: true }], { dialect: 'plsql', strict: true, grammar: 'Oracle' }, 'Oracle')
    expect(m.message).toBe('语句不完整')
    expect(m.endColumn).toBe(6)
  })
})
```

- [ ] **Step 2: 修改 `web/src/app/SettingsDialog.tsx`**

```diff
--- a/web/src/app/SettingsDialog.tsx
+++ b/web/src/app/SettingsDialog.tsx
@@ -1,10 +1,11 @@
-// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限，桌面版另有"启动时自动检查更新"。
+// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限、语法检查，桌面版另有"启动时自动检查更新"。
 // macOS 上没有页面菜单，主题只能在这里改。
 
 import { Form, InputNumber, Modal, Radio, Switch } from 'antd'
 import { useTranslation } from 'react-i18next'
 
 import { DEFAULT_ROW_CAP, ROW_CAP_KEY, rowCap } from '../features/editor/runner'
+import { SYNTAX_KEY, syntaxEnabled } from '../features/sql/syntax'
 import { LANG_KEY, parseLangPref } from '../shared/i18n'
 import { useSetting, useSettings } from '../shared/settings'
 import { desktop } from '../shared/shell'
@@ -18,6 +19,7 @@ export default function SettingsDialog() {
   const theme = parseThemePref(useSetting(THEME_KEY))
   const lang = parseLangPref(useSetting(LANG_KEY))
   useSetting(ROW_CAP_KEY)
+  const syntax = syntaxEnabled(useSetting(SYNTAX_KEY))
   const info = useShellInfo()
   return (
     <Modal title={t('app.settings.title')} open={settingsOpen} onCancel={() => setSettingsOpen(false)} footer={null} width={480}>
@@ -54,6 +56,9 @@ export default function SettingsDialog() {
             style={{ width: 160 }}
           />
         </Form.Item>
+        <Form.Item label={t('app.settings.syntaxCheck')} extra={t('app.settings.syntaxCheckHint')}>
+          <Switch checked={syntax} onChange={(on) => useSettings.getState().set(SYNTAX_KEY, on ? null : 'off')} />
+        </Form.Item>
         {desktop && (
           <Form.Item label={t('app.settings.autoCheckUpdate')}>
             <Switch
```

- [ ] **Step 3: 修改 `web/src/app/StatusBar.tsx`**

```diff
--- a/web/src/app/StatusBar.tsx
+++ b/web/src/app/StatusBar.tsx
@@ -1,15 +1,17 @@
 // 状态栏（M1 设计文档 1.1 节）：有查询标签时显示它的连接和方言、事务状态、最近一次执行的行数和耗时、
-// 光标位置；没有时显示对象树中选中的连接。
+// 语法问题数（点击跳到第一处）、光标位置；没有时显示对象树中选中的连接。
 
-import { IconPlugConnected } from '@tabler/icons-react'
+import { IconAlertTriangle, IconPlugConnected } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
 import { useTranslation } from 'react-i18next'
 
 import { driverLabel } from '../features/connections/form'
+import { sendCommand } from '../features/editor/commands'
 import { useCursor } from '../features/editor/cursor'
 import { variantName } from '../features/editor/dialect'
 import { useTabRuntime } from '../features/editor/runner'
 import { useTabs } from '../features/editor/tabs'
+import { useSyntax } from '../features/sql/syntax'
 import DbIcon from '../shared/DbIcon'
 import { lastSummary } from '../features/results/results'
 import type { Profile } from '../shared/api'
@@ -23,6 +25,7 @@ function TabStatus({ tabId, profile }: { tabId: string; profile: Profile | undef
   const s = rt.session
   const dialect = variantName(s?.variant) ?? (profile && driverLabel(profile.driver))
   const summary = lastSummary(rt.exec)
+  const syntax = useSyntax((x) => x.tabs[tabId])
   return (
     <>
       <span className="row" style={{ gap: 4 }}>
@@ -41,6 +44,14 @@ function TabStatus({ tabId, profile }: { tabId: string; profile: Profile | undef
       )}
       {rt.running ? <span>{t('app.status.running')}</span> : summary && <span>{summary}</span>}
       <span className="grow" />
+      {syntax?.status === 'ok' && syntax.count > 0 && (
+        <span className="row" style={{ gap: 4, color: 'var(--warn)', cursor: 'pointer' }} onClick={() => sendCommand('first-problem')}>
+          <IconAlertTriangle size={14} />
+          {t('app.status.syntaxIssues', { count: syntax.count })}
+        </span>
+      )}
+      {syntax?.status === 'stopped' && <span className="muted">{t('app.status.syntaxStopped')}</span>}
+      {syntax?.status === 'skipped' && <span className="muted">{t('app.status.syntaxSkipped')}</span>}
       <span>{t('app.status.cursor', { line: cursor.line, column: cursor.column })}</span>
       {dialect && <span>{dialect}</span>}
       <span>UTF-8</span>
```

- [ ] **Step 4: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -12,6 +12,7 @@ import { type Profile, errorText } from '../../shared/api'
 import { layoutStorage } from '../../shared/settings'
 import ResultsPanel from '../results/ResultsPanel'
 import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
+import { useSyntax } from '../sql/syntax'
 import ConfirmDialog from './ConfirmDialog'
 import Toolbar from './Toolbar'
 import { onCommand } from './commands'
@@ -68,6 +69,16 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
           case 'cancel':
             runner.cancel()
             break
+          case 'first-problem': {
+            const first = useSyntax.getState().tabs[tab.id]?.first
+            if (first) {
+              const pos = { lineNumber: first.line, column: first.column }
+              ed.setPosition(pos)
+              ed.revealPositionInCenter(pos)
+              ed.focus()
+            }
+            break
+          }
           case 'format': {
             if (!profile) return
             const range = selected ?? model.getFullModelRange()
@@ -81,7 +92,7 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
           }
         }
       }),
-    [runner, profile, rt.session?.variant, message, t],
+    [runner, profile, rt.session?.variant, message, t, tab.id],
   )
 
   // 协议层面的错误（如上一段还没执行完）用全局提示（M1 设计文档第 5 节），同时记在"消息"里
```

- [ ] **Step 5: 修改 `web/src/features/editor/SqlEditor.tsx`**

```diff
--- a/web/src/features/editor/SqlEditor.tsx
+++ b/web/src/features/editor/SqlEditor.tsx
@@ -1,6 +1,6 @@
 // SQL 编辑器：Monaco。每个标签页一个模型（以标签 id 为路径），切换标签时保留各自的撤销历史和光标。
 // 快捷键（M1 设计文档 1.3 节）注册为编辑器动作，焦点在编辑器里时由 Monaco 处理；错误标记来自执行结果。
-// 补全（M1b2）：模型绑定所在标签页的连接上下文，由 features/sql/provider.ts 提供补全项。
+// 补全与语法检查（M1b2）：模型绑定所在标签页的连接上下文，由 features/sql 中的 provider 和 checker 处理。
 
 import Editor, { type OnMount } from '@monaco-editor/react'
 import { useEffect, useRef } from 'react'
@@ -8,6 +8,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useSetting } from '../../shared/settings'
 import { THEME_KEY, parseThemePref, resolveTheme, useSystemDark } from '../../shared/theme'
+import { attachChecker } from '../sql/checker'
 import { type EditorSqlContext, bindModel, registerSqlProviders } from '../sql/provider'
 import { type EditorCommand, sendCommand } from './commands'
 import { useCursor } from './cursor'
@@ -41,16 +42,21 @@ export interface SqlEditorProps {
 }
 
 export default function SqlEditor({ path, language, defaultValue, marks, sql, onChange, onReady }: SqlEditorProps) {
-  const { t } = useTranslation()
+  const { t, i18n } = useTranslation()
   const systemDark = useSystemDark()
   const theme = resolveTheme(parseThemePref(useSetting(THEME_KEY)), systemDark)
   const editorRef = useRef<MonacoEditor | null>(null)
   const sqlRef = useRef(sql)
   sqlRef.current = sql
   useEffect(() => bindModel(path, () => sqlRef.current), [path])
+  const checker = useRef<ReturnType<typeof attachChecker> | null>(null)
+  useEffect(() => () => checker.current?.dispose(), [])
+  // 识别出变体（标准档 / 兼容档）、换了连接或界面语言时重新检查
+  useEffect(() => checker.current?.recheck(), [sql?.driver, sql?.variant, i18n.language])
 
   const onMount: OnMount = (editor) => {
     editorRef.current = editor
+    checker.current = attachChecker(editor)
     for (const a of ACTIONS) {
       editor.addAction({
         id: `dblens.${a.id}`,
```

- [ ] **Step 6: 修改 `web/src/features/editor/commands.ts`**

```diff
--- a/web/src/features/editor/commands.ts
+++ b/web/src/features/editor/commands.ts
@@ -1,6 +1,6 @@
 // 编辑器命令：菜单、全局快捷键和工具栏都发命令，由当前标签页的视图执行（它持有编辑器实例）。
 
-export type EditorCommand = 'run-current' | 'run-all' | 'cancel' | 'format' | 'explain'
+export type EditorCommand = 'run-current' | 'run-all' | 'cancel' | 'format' | 'explain' | 'first-problem'
 
 const bus = new EventTarget()
 
```

- [ ] **Step 7: 新建 `web/src/features/sql/checker.ts`**

```ts
// 编辑时的语法检查（M1b2 设计文档第 5、7 节）：
// - 编辑后 400 ms 把整个脚本交给 worker 检查，结果标在编辑器里（与执行出错的标记来源不同，互不覆盖）；
// - 设置关闭、连接已删除、解析器不可用时不检查；脚本超过 1 MB 时不检查；
// - 单次超过 2 秒丢弃结果，连续三次超时后该标签页停止检查（状态栏提示）。

import { driverLabel } from '../connections/form'
import { variantName } from '../editor/dialect'
import { monaco } from '../editor/monaco'
import { currentLang } from '../../shared/i18n'
import { useSettings } from '../../shared/settings'
import { sqlDialect } from './dialects'
import { syntaxMarks } from './diagnostics'
import { contextOf, pathOf } from './provider'
import { sqlClient } from './service'
import { NO_SYNTAX, SYNTAX_KEY, type SyntaxState, syntaxEnabled, useSyntax } from './syntax'

const OWNER = 'dblens-syntax'
export const DEBOUNCE_MS = 400
export const MAX_CHARS = 1_000_000
const MAX_TIMEOUTS = 3

type Editor = monaco.editor.IStandaloneCodeEditor
type Model = monaco.editor.ITextModel

/** 每个模型连续超时的次数；达到上限后不再检查。 */
const timeouts = new WeakMap<Model, number>()

async function check(model: Model) {
  const path = pathOf(model)
  if (!path || model.isDisposed()) return
  const report = (s: SyntaxState) => useSyntax.getState().set(path, s)
  const clear = (s: SyntaxState) => {
    monaco.editor.setModelMarkers(model, OWNER, [])
    report(s)
  }
  const ctx = contextOf(model)
  if (!ctx || !syntaxEnabled(useSettings.getState().values[SYNTAX_KEY])) return clear(NO_SYNTAX)
  if ((timeouts.get(model) ?? 0) >= MAX_TIMEOUTS) return clear({ ...NO_SYNTAX, status: 'stopped' })
  const text = model.getValue()
  if (text.length > MAX_CHARS) return clear({ ...NO_SYNTAX, status: 'skipped' })
  const d = sqlDialect(ctx.driver, ctx.variant)
  if (sqlClient.unavailable(d.dialect)) return clear(NO_SYNTAX)

  const version = model.getVersionId()
  const r = await sqlClient.validate(d.dialect, text, currentLang() === 'zh-CN' ? 'zh_CN' : 'en_US')
  // 检查期间又改过：结果作废（改动会触发新的检查）
  if (model.isDisposed() || model.getVersionId() !== version) return
  if (!r.ok) {
    if (r.reason === 'timeout') {
      const n = (timeouts.get(model) ?? 0) + 1
      timeouts.set(model, n)
      if (n >= MAX_TIMEOUTS) clear({ ...NO_SYNTAX, status: 'stopped' })
    } else clear(NO_SYNTAX)
    return
  }
  timeouts.delete(model)
  const name = variantName(ctx.variant) ?? driverLabel(ctx.driver)
  const marks = syntaxMarks(r.value, d, name)
  monaco.editor.setModelMarkers(
    model,
    OWNER,
    marks.map((m) => ({ ...m, severity: m.severity === 'error' ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning })),
  )
  const first = marks[0] ? { line: marks[0].startLineNumber, column: marks[0].startColumn } : null
  report({ status: 'ok', count: marks.length, first })
}

/**
 * 给编辑器装上语法检查：内容变化、切换模型时检查当前模型。
 * 返回 recheck（上下文、设置、语言变化时调用）和 dispose。
 */
export function attachChecker(editor: Editor) {
  let timer: ReturnType<typeof setTimeout> | undefined
  const schedule = (delay = DEBOUNCE_MS) => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const model = editor.getModel()
      if (model) void check(model)
    }, delay)
  }
  const subs = [editor.onDidChangeModelContent(() => schedule()), editor.onDidChangeModel(() => schedule(0))]
  const unsubscribe = useSettings.subscribe((s, prev) => {
    if (s.values[SYNTAX_KEY] !== prev.values[SYNTAX_KEY]) schedule(0)
  })
  schedule(0)
  return {
    recheck: () => schedule(0),
    dispose: () => {
      clearTimeout(timer)
      unsubscribe()
      for (const s of subs) s.dispose()
    },
  }
}
```

- [ ] **Step 8: 新建 `web/src/features/sql/diagnostics.ts`**

```ts
// 语法问题 → 编辑器标记（M1b2 设计文档第 5 节）：标准库标为错误（红），兼容库标为警告（黄）并说明按哪种语法检查。

import { t } from '../../shared/i18n'
import type { SqlDialect } from './dialects'
import type { ParseIssue } from './types'

export interface SyntaxMark {
  severity: 'error' | 'warning'
  message: string
  startLineNumber: number
  startColumn: number
  endLineNumber: number
  endColumn: number
}

/** name：数据库的名称（变体名或驱动名），兼容档的提示中使用。 */
export function syntaxMarks(issues: ParseIssue[], d: SqlDialect, name: string): SyntaxMark[] {
  return issues.map((i) => {
    let message = i.message ?? (i.incomplete ? t('editor.syntax.incomplete') : t('editor.syntax.near', { near: i.near ?? '' }))
    if (!d.strict) message += t('editor.syntax.compatHint', { grammar: d.grammar, name })
    return {
      severity: d.strict ? 'error' : 'warning',
      message,
      startLineNumber: i.startLine,
      startColumn: i.startColumn,
      endLineNumber: i.endLine,
      // 至少标出一个字符
      endColumn: i.endLine === i.startLine ? Math.max(i.endColumn, i.startColumn + 1) : i.endColumn,
    }
  })
}
```

- [ ] **Step 9: 修改 `web/src/features/sql/provider.ts`**

```diff
--- a/web/src/features/sql/provider.ts
+++ b/web/src/features/sql/provider.ts
@@ -16,19 +16,30 @@ export interface EditorSqlContext {
   meta: MetaReader
 }
 
-const contexts = new Map<string, () => EditorSqlContext | null>()
+interface Binding {
+  path: string
+  get: () => EditorSqlContext | null
+}
+
+const bindings = new Map<string, Binding>()
 
-/** 把编辑器模型（按 path）绑定到上下文；返回解绑函数。 */
+/** 把编辑器模型（按 path，即标签页 id）绑定到上下文；返回解绑函数。 */
 export function bindModel(path: string, get: () => EditorSqlContext | null): () => void {
   const key = monaco.Uri.parse(path).toString()
-  contexts.set(key, get)
+  const b = { path, get }
+  bindings.set(key, b)
   return () => {
-    if (contexts.get(key) === get) contexts.delete(key)
+    if (bindings.get(key) === b) bindings.delete(key)
   }
 }
 
 export function contextOf(model: monaco.editor.ITextModel): EditorSqlContext | null {
-  return contexts.get(model.uri.toString())?.() ?? null
+  return bindings.get(model.uri.toString())?.get() ?? null
+}
+
+/** 模型所属的标签页 id。 */
+export function pathOf(model: monaco.editor.ITextModel): string | null {
+  return bindings.get(model.uri.toString())?.path ?? null
 }
 
 const { CompletionItemKind: K } = monaco.languages
```

- [ ] **Step 10: 新建 `web/src/features/sql/syntax.ts`**

```ts
// 语法检查的设置和每个标签页的结果（状态栏显示）。单独放一个文件，状态栏和设置不必引入 Monaco。

import { create } from 'zustand'

/** 设置项：编辑时检查语法，默认开；值为 off 时关闭。 */
export const SYNTAX_KEY = 'ui.editor.syntax-check'

export const syntaxEnabled = (raw: string | undefined) => raw !== 'off'

export interface SyntaxState {
  /** ok：已检查；stopped：连续超时，已停止；skipped：脚本过大，未检查；off：没有检查（关闭或解析器不可用）。 */
  status: 'ok' | 'stopped' | 'skipped' | 'off'
  count: number
  /** 第一处问题的位置，状态栏点击时跳过去。 */
  first: { line: number; column: number } | null
}

export const NO_SYNTAX: SyntaxState = { status: 'off', count: 0, first: null }

interface SyntaxStore {
  tabs: Record<string, SyntaxState>
  set(tabId: string, s: SyntaxState): void
}

export const useSyntax = create<SyntaxStore>()((set, get) => ({
  tabs: {},
  set: (tabId, s) => set({ tabs: { ...get().tabs, [tabId]: s } }),
}))
```

- [ ] **Step 11: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -11,7 +11,9 @@
     "theme": "Theme",
     "rowCap": "Max result rows",
     "rowCapHint": "Stop reading a result set after this many rows",
-    "autoCheckUpdate": "Check for updates at startup"
+    "autoCheckUpdate": "Check for updates at startup",
+    "syntaxCheck": "Syntax check",
+    "syntaxCheckHint": "Mark syntax problems while editing; databases compatible with MySQL / PostgreSQL / Oracle get yellow hints only. Does not affect execution"
   },
   "theme": {
     "system": "System",
@@ -37,7 +39,11 @@
     "inTransaction": "Manual commit · In transaction",
     "running": "Running…",
     "cursor": "Ln {{line}}, Col {{column}}",
-    "noConnection": "No connection selected"
+    "noConnection": "No connection selected",
+    "syntaxIssues_one": "{{count}} syntax issue",
+    "syntaxIssues_other": "{{count}} syntax issues",
+    "syntaxStopped": "Syntax check stopped (script too large or complex)",
+    "syntaxSkipped": "Script over 1 MB, syntax not checked"
   },
   "titleBar": {
     "windowTitle": "{{tab}} · {{connection}} — {{app}}",
```

- [ ] **Step 12: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -54,5 +54,10 @@
   },
   "dialect": {
     "dameng": "DM"
+  },
+  "syntax": {
+    "incomplete": "Statement is incomplete",
+    "near": "Syntax error near '{{near}}'",
+    "compatHint": " (checked with {{grammar}} grammar; {{name}} extensions may be flagged by mistake)"
   }
 }
```

- [ ] **Step 13: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -11,7 +11,9 @@
     "theme": "主题",
     "rowCap": "结果行数上限",
     "rowCapHint": "一个结果集读到这么多行后停止读取",
-    "autoCheckUpdate": "启动时自动检查更新"
+    "autoCheckUpdate": "启动时自动检查更新",
+    "syntaxCheck": "语法检查",
+    "syntaxCheckHint": "编辑时标出语法问题；兼容 MySQL / PostgreSQL / Oracle 的国产库只标黄色提示。不影响执行"
   },
   "theme": {
     "system": "跟随系统",
@@ -37,7 +39,10 @@
     "inTransaction": "手动提交 · 事务中",
     "running": "执行中…",
     "cursor": "行 {{line}}，列 {{column}}",
-    "noConnection": "未选择连接"
+    "noConnection": "未选择连接",
+    "syntaxIssues": "{{count}} 处语法问题",
+    "syntaxStopped": "语法检查已停止（脚本过大或过于复杂）",
+    "syntaxSkipped": "脚本超过 1 MB，未检查语法"
   },
   "titleBar": {
     "windowTitle": "{{tab}} · {{connection}} — {{app}}",
```

- [ ] **Step 14: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -54,5 +54,10 @@
   },
   "dialect": {
     "dameng": "达梦"
+  },
+  "syntax": {
+    "incomplete": "语句不完整",
+    "near": "“{{near}}”附近有语法错误",
+    "compatHint": "（按 {{grammar}} 语法检查，{{name}} 的扩展语法可能被误报）"
   }
 }
```

- [ ] **Step 15: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；27 个测试文件、176 个测试通过（新增 diagnostics 3 个）；打包成功。

- [ ] **Step 16: 提交**

```bash
git add web/src/app/SettingsDialog.tsx web/src/app/StatusBar.tsx web/src/features/editor/QueryView.tsx web/src/features/editor/SqlEditor.tsx web/src/features/editor/commands.ts web/src/features/sql/checker.ts web/src/features/sql/diagnostics.test.ts web/src/features/sql/diagnostics.ts web/src/features/sql/provider.ts web/src/features/sql/syntax.ts web/src/locales/en/app.json web/src/locales/en/editor.json web/src/locales/zh-CN/app.json web/src/locales/zh-CN/editor.json
git commit -m "feat(web): 编辑时语法检查：标准库标红、兼容库标黄并说明，设置可关，状态栏显示问题数并可跳转" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 说明文档与实测

写 M1b2 说明文档（技术验证数据、语法补丁、已知误报、实现中的决定），M1 设计文档的进度改为"M1b2 已完成"。然后用浏览器模式实测。执行时若任务 1 的依赖复查结果与写计划时不同，按实际结果修改说明文档的"依赖"一节。

**Files:**
- Create: `docs/notes/m1b2-completion.md`
- Modify: `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`

- [ ] **Step 1: 新建 `docs/notes/m1b2-completion.md`**

````markdown
# M1b2 SQL 补全与语法检查

依据：设计文档 `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`。

## 结构

```text
scripts/check-deps.mjs            依赖复查：列出可升级的直接依赖（前端和 Cargo）
web/grammars/plsql/               grammars-v4 的 PL/SQL 语法（上游原样，README 记来源和提交号）
web/scripts/gen-plsql.mjs         打达梦补丁、用 antlr-ng 生成解析器到 src/features/sql/plsql/gen/（不提交）
web/src/features/sql/
  types.ts                        worker 与主线程之间的类型：ParseIssue、Suggest、SqlParser
  plsql/parser.ts                 Oracle、达梦：分句、逐句解析、antlr4-c3 补全分析、按词扫描表和别名
  dtsql.ts · mysql.ts · postgres.ts   MySQL 系、PG 系：dt-sql-parser 的适配，各自懒加载
  parsers.ts · worker.ts · protocol.ts  worker：按方言加载解析器，validate / suggest
  client.ts · service.ts          主线程：请求序号、2 秒超时、加载失败与 worker 出错的处理
  dialects.ts                     驱动 / 变体 → 解析器与档位（标准红 / 兼容黄）
  metadata.ts                     补全用的元数据（react-query 缓存，键 ['meta', 连接, schema, …]）
  completion.ts · functions.ts    补全项：列、表、schema、函数、关键字；轻量补全
  provider.ts                     Monaco 补全提供者；编辑器模型绑定标签页上下文
  diagnostics.ts · checker.ts · syntax.ts   语法检查的标记、编辑时检查、设置和状态栏数据
```

## 方言与档位

| 驱动 / 变体 | 解析器 | 档位 |
|---|---|---|
| MySQL、MariaDB | dt-sql-parser MySQL | 标准（红） |
| TiDB、OceanBase、PolarDB | dt-sql-parser MySQL | 兼容（黄） |
| PostgreSQL | dt-sql-parser PostgreSQL | 标准（红） |
| KingbaseES、openGauss、GaussDB、HighGo | dt-sql-parser PostgreSQL | 兼容（黄） |
| Oracle | PL/SQL | 标准（红） |
| 达梦 | PL/SQL | 兼容（黄） |

没连接过的配置不知道变体，按兼容档处理；连接后（执行或展开对象树）自动换成对应档位。

## Oracle / 达梦技术验证（2026-10-26）

语法：grammars-v4 `sql/plsql`，提交 `afbe4ed460d091afd7158b5cbd4ef63e64523e21`（2026-10-17）。工具：`antlr-ng` 1.0.10、运行时 `antlr4ng` 3.0.16、`antlr4-c3` 3.4.4。

| 项目 | 合格线 | 实测 | 结论 |
|---|---|---|---|
| 打包体积 | gzip ≤ 3 MB | 压缩后 6.7 MB，gzip 1.42 MB | 达标 |
| 1000 行脚本解析 | ≤ 300 ms | 1025 行：首次 1.2 s（建立预测缓存），之后 94–147 ms | 达标 |
| 补全命中 | ≥ 80% | 20 个样例命中 19 个（95%），单次 0.1–12 ms | 达标 |
| 达梦误报 | 记录 | 测试夹具 51 条：补丁前 25 条报错，补丁后 2 条 | 记录 |
| Oracle 误报 | — | 测试夹具 80 条：补丁前 3 条，补丁后 2 条 | 记录 |

结论：达标，Oracle 和达梦走完整流程（Oracle 标红，达梦标黄）。

对比：dt-sql-parser 的 MySQL 解析器 gzip 0.35 MB，PG 0.31 MB；1000 行 MySQL 脚本首次检查 366 ms，之后有缓存。

验证中的调整（与设计文档不同之处）：
- 生成工具用 `antlr-ng`（纯 TypeScript，不需要 Java），生成约 5 秒，所以生成的代码不提交，`pnpm dev` / `build` / `test` / `typecheck` 前自动生成，输入不变时跳过。
- 按 Oracle 的规则先分句再逐句解析（与后端 `dblens-dialect` 的 split.rs 一致）：一句出错不影响后面的检查；补全只解析光标所在的一句；没改动的句子用缓存。
- 表和别名按词扫描（FROM / JOIN / INTO / UPDATE / USING / TABLE 之后），写到一半的语句也能识别。

### 语法补丁

在 `web/scripts/gen-plsql.mjs` 的 `PATCHES` 中，每条在 `plsql/parser.test.ts` 有对应语句：
- `DROP <对象> IF EXISTS 名称`（函数、包、过程、触发器、序列、物化视图、表、视图、同义词；Oracle 23ai 也支持）；
- 达梦：`DROP TABLE … CASCADE`、`IDENTITY(种子, 增量)`、`COMMENT ON VIEW`、`BIGINT` / `TINYINT` / `BYTE` / `TEXT` / `TIME` 类型、`VARBINARY(n)` 等带长度的类型名；
- 分区边界中的 `DATE '…'` 字面量（Oracle、达梦）。

### 已知误报

- 达梦：`SELECT TOP n …`、`INTERVAL YEAR` 不带 `TO MONTH`、`TIME WITH TIME ZONE` 等（标黄，提示说明可能误报）。
- Oracle：复合触发器（`COMPOUND TRIGGER`）中的部分写法、23ai 的 `VECTOR(n, FLOAT32)`（上游语法的缺陷）。

## 依赖

- 依赖原则：只用正式版，跟随最新，新版发布满 7 天再升级，严重安全修复不等。每个里程碑开始时运行：
  `node scripts/check-deps.mjs`（国内可设 `NPM_REGISTRY=https://registry.npmmirror.com`）。
- 2026-10-26 复查：升级 i18next 26.5.2、react-i18next 17.1.0；antd 6.6.8、react-resizable-panels 4.15.0、uuid（Cargo）1.29.0 发布未满 7 天，下次复查。
- dt-sql-parser 4.5.1 自身依赖 `antlr4ng` 2.0.11（4.5.2 只有 beta）。MySQL / PG 的块和 PL/SQL 的块各带一份 ANTLR 运行时，各自懒加载；等它的正式版升到 `antlr4ng` 3 后合并。
- vite 的 worker 输出改为 ES 模块（`worker.format: 'es'`），否则 worker 里的动态 import 不能拆块，所有解析器会打进一个 10 MB 的文件。

## 实现中的决定

- 所有方言共用一个 worker，解析器在 worker 内按需加载；某个方言第一次使用时先加载并预热（不计超时），之后每次请求 2 秒超时。
- 补全请求发整个脚本和光标偏移，由解析器自己找光标所在的语句（dt-sql-parser 和 PL/SQL 都只解析那一句）。
- 元数据：补全最多等 1.5 秒，超时先给已有的，请求在后台完成，下次补全就有。
- 执行成功的语句中有 DDL（CREATE、ALTER、DROP、RENAME、TRUNCATE、COMMENT）时，清掉该连接的后端元数据缓存和补全缓存；对象树仍需手动刷新。
- 设置项名为 `ui.editor.syntax-check`（设置名只允许小写字母、数字和 `._-`），值为 `off` 时关闭。
- 补全插入的名称按方言在需要时加引号：MySQL 系含特殊字符时用反引号；PG 系不是全小写时、Oracle 和达梦不是全大写时用双引号。
- 编辑器关闭了"补全文中出现过的词"（`wordBasedSuggestions: 'off'`），只用 SQL 补全。
- dt-sql-parser 补全时会把中间解析的报错打印到控制台，适配层调用期间临时屏蔽 `console.error`。

## 验证

- 前端：tsc 通过；27 个测试文件、176 个测试通过；打包通过，解析器各自成块。
- 浏览器实测（5 个实库，中英文界面）：
  - MariaDB：`FROM` 后补 schema 和关键字；建表后 `e.` 补出该表的列和类型；执行 DROP 后自动刷新元数据。
  - Oracle：表带注释；`d.` 补出列和注释；识别出 Oracle 后语法错误标红。
  - 达梦：表、视图、同义词、中文表名；`SELECT TOP 5` 标黄并说明可能误报。
  - PostgreSQL：`n.` 补出 `pg_catalog.pg_namespace` 的列。
  - 英文界面：状态栏 “1 syntax issue”；设置中关闭语法检查后标记和状态栏提示消失，再打开立即恢复。
- 实测中 PG 标签有一次补全显示“无建议”（未发出元数据请求），重新加载后未能复现，留意后续使用。
````

- [ ] **Step 2: 修改 `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
+++ b/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
@@ -155,6 +155,7 @@ M1b 在写计划时（2026-10-25，用户确认）再拆成两份：
 - M1b1 已完成（详细计划 `docs/superpowers/plans/2026-10-25-m1b1-editor.md`，说明 `docs/notes/m1b1-editor.md`）。
 - 2026-10-25 在 M1b2 之前插入 M1i：中英双语与数据库图标（设计 `docs/superpowers/specs/2026-10-25-m1i-i18n-icons-design.md`）。
 - M1i 已完成（详细计划 `docs/superpowers/plans/2026-10-25-m1i-i18n-icons.md`，说明 `docs/notes/m1i-i18n.md`）。
+- M1b2 已完成（设计 `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1b2-completion.md`，说明 `docs/notes/m1b2-completion.md`）。
 
 右键菜单中的"查看 DDL"和"生成 SQL"依赖 M1c 的后端，M1a 先不显示这两项。
 
```

- [ ] **Step 3: 检查**

全量回归：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

实测（浏览器模式，用新的数据目录启动 `dblens-server --web-dir web/dist`）：
1. 建 5 个测试连接（密码只经环境变量传给建连接的脚本），确认 Oracle、达梦的客户端库目录。
2. MariaDB：`SELECT * FROM ` 后 Ctrl+Space 列出 schema 和关键字；建两张临时表后（执行 DDL 后自动刷新）`SELECT e.| FROM t1 e JOIN t2 d ON …` 补出 t1 的列和类型，按表中顺序。
3. Oracle：`FROM` 后列出表和注释；别名补出列；执行一条语句识别出 Oracle 后，`SELECT * FORM t` 标红，悬停提示"“FORM”附近有语法错误"。
4. 达梦：表、视图、同义词、中文表名都能补全；`SELECT TOP 5 * FROM t` 标黄，提示中说明"按 Oracle 语法检查，达梦 的扩展语法可能被误报"。
5. PostgreSQL：`SELECT n.| FROM pg_catalog.pg_namespace n` 补出 4 列。
6. 设置切到 English：状态栏为 "1 syntax issue"；关闭语法检查后标记和状态栏提示消失，再打开立即恢复。
7. 首次打开某种库的标签时，开发者工具中只下载该方言的解析器块。
8. 删除临时表和测试连接（系统凭据中的密码随之删除），停掉服务。

Expected: 全部通过。

- [ ] **Step 4: 提交**

```bash
git add docs/notes/m1b2-completion.md docs/superpowers/specs/2026-10-25-m1-workbench-design.md
git commit -m "docs: M1b2 说明文档：技术验证数据、语法补丁、已知误报与实现中的决定" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
