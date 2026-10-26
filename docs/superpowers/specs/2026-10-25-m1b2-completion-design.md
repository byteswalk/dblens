# M1b2 SQL 补全与语法检查 设计

日期：2026-10-25。上级设计：`docs/superpowers/specs/2026-10-25-m1-workbench-design.md`（2.1 节选型、第 4 节里程碑划分）。前置：M1b1（编辑与执行）、M1i（中英双语）已完成。

## 1. 目标与范围

在 SQL 编辑器中提供：
- 上下文补全：关键字、schema、表和视图、列（含 `别名.`）、常用内置函数；Oracle 和达梦另有同义词、序列。
- 语法检查：编辑时在出错位置标波浪线。
- Oracle / 达梦解析器的技术验证，按结果决定这两种库做完整补全还是轻量补全。

不做：代码片段；按语义检查对象是否存在；后端语法检查；修改现有 Monarch 高亮。

## 2. 已定的决策（2026-10-25 与用户确认）

| 问题 | 结论 |
|---|---|
| Oracle / 达梦 | 先做技术验证（第 6 节），达标就完整做，不达标退回轻量补全 |
| 兼容库的误报 | 分两档：标准库标红色错误，兼容库标黄色"可能有误"；设置可关；都不阻止执行 |
| 依赖版本 | 只用正式发布版，跟随最新；新版发布满 7 天再升级，严重安全修复不等（第 3.1 节） |
| 架构 | 方案一：自建 SQL 语言 worker，统一包住各方言解析器；不用 `monaco-sql-languages`（会覆盖高亮，诊断级别和语言不可控，没有 Oracle），也不在后端检查 |

## 3. 依赖与选型

### 3.1 依赖原则（全项目适用，用户 2026-10-25 要求）

- 只用正式发布版：不用 alpha、beta、rc、next 等预发布版本或标签。
- 跟随最新：使用最新的正式版；新版发布满 7 天再升级（避开刚发布就被撤回或投毒的版本）；严重安全修复不等。
- 每个里程碑的实施计划，第一个任务都是依赖复查：前端（npm）和后端（Cargo）的全部直接依赖升级到符合上两条的最新版，跑全量测试后单独提交。
- 不能升级时（上游有破坏性变化尚未适配、或间接依赖被上游固定），在该里程碑的说明文档里记下原因和复查条件。
- 没有发布版的来源（如 grammars-v4 语法）固定到某个提交，记录提交号，每个里程碑复查一次。

2026-10-25 复查：前端 27 个直接依赖中 4 个落后（antd 6.6.8、i18next 26.5.11、react-i18next 17.1.9、react-resizable-panels 4.15.0），新版都未满 7 天，在 M1b2 第一个任务时再按规则处理。

### 3.2 选型

2026-10-25 查询 npm：

| 库 | 版本 | 许可证 | 用途 |
|---|---|---|---|
| `dt-sql-parser` | 4.5.1（2026-08-25 发版） | MIT | MySQL、PostgreSQL 的解析器：`validate`、`getSuggestionAtCaretPosition`、`getAllEntities`；自带 `zh_CN` / `en_US` 错误文字 |
| `antlr4ng` | 3.0.16 | BSD-3 | Oracle 解析器的运行时 |
| `antlr4-c3` | 3.4.4 | MIT | Oracle 解析器的补全候选分析（依赖 `antlr4ng` 3.0.16） |
| `antlr-ng` | 1.0.10 | BSD-3 | ANTLR 工具的 TypeScript 版，生成 `antlr4ng` 3 的解析器；开发依赖，不需要 Java |
| grammars-v4 `sql/plsql` | 验证时取最新提交，记录提交号 | Apache-2.0 | PL/SQL 语法 |

`dt-sql-parser` 的 MySQL、PostgreSQL 解析器未压缩各约 4.8 MB，必须按方言懒加载。

`dt-sql-parser` 4.5.1 自身依赖 `antlr4ng` 2.0.11 和 `antlr4-c3` 3.3.7（它的 4.5.2 只有 beta），所以 MySQL / PG 解析器块和 Oracle 解析器块各带一份 ANTLR 运行时。两块都按需加载，只在同时用到两类方言时才都下载，代价约一百多 KB。等 `dt-sql-parser` 的正式版升到 `antlr4ng` 3 后自然合并；按 3.1 节记入说明文档。

方言对应：

| 驱动 / 变体 | 解析器 | 档位 |
|---|---|---|
| MySQL、MariaDB | dt-sql-parser MySQL | 标准（红） |
| TiDB、OceanBase、PolarDB | dt-sql-parser MySQL | 兼容（黄） |
| PostgreSQL | dt-sql-parser PostgreSQL | 标准（红） |
| KingbaseES、openGauss、GaussDB、HighGo | dt-sql-parser PostgreSQL | 兼容（黄） |
| Oracle | PL/SQL（验证通过后） | 标准（红） |
| 达梦 | PL/SQL（验证通过后） | 兼容（黄） |

变体未知（还没连接过）时按驱动取解析器，按兼容档处理。

## 4. 结构与数据流

```text
web/src/features/sql/
  worker.ts          SQL 语言 worker：按方言懒加载解析器，提供 validate / suggest
  parsers.ts         方言 → 解析器的加载与适配（worker 内使用；Node 测试直接调用）
  client.ts          主线程封装：所有方言共用一个 worker，懒创建；请求带序号，超时的结果丢弃
  dialects.ts        驱动 / 变体 → 解析器与档位（第 3 节的表）
  completion.ts      worker 的补全需求 + 元数据 → Monaco 补全项（纯逻辑）
  metadata.ts        元数据：复用 react-query 缓存，按需取 schema 对象和表的列
  diagnostics.ts     语法错误 → Monaco 标记（档位、开关、提示文字）
  functions/<方言>.ts  精简的内置函数表（mysql、postgres、oracle）
  plsql/             Oracle / 达梦解析器的适配；gen/ 为生成产物（不提交）
web/grammars/plsql/          上游 PL/SQL 语法
web/scripts/gen-plsql.mjs    打补丁并生成 PL/SQL 解析器
```

worker 的接口：

```ts
type Dialect = 'mysql' | 'postgres' | 'plsql'
validate(sql: string, dialect: Dialect, locale: 'zh_CN' | 'en_US'): ParseIssue[]
suggest(statement: string, caret: { lineNumber: number; column: number }, dialect: Dialect): Suggest
interface ParseIssue { startLine; startColumn; endLine; endColumn; message: string }
interface Suggest {
  keywords: string[]
  want: Array<'schema' | 'table' | 'view' | 'column' | 'function'>   // 光标处该出现的对象类型
  qualifier?: string[]            // 光标前的限定名，如 `a.`、`hr.emp.`
  tables: Array<{ schema?: string; name: string; alias?: string }>  // 当前语句里出现的表
}
```

- 语法检查：编辑后防抖 400 ms，整份脚本发给 worker；解析器按 `;` 分句，每句单独报错。脚本超过 1 MB 时不检查。
- 补全：Monaco 请求补全时，把整个脚本和光标位置发给 worker，由解析器找到光标所在的语句，只解析这一句（前端没有现成的语句定位，dt-sql-parser 自带，PL/SQL 按后端 split.rs 的规则分句）。主线程按 `want` 和 `qualifier` 查元数据：
  - `qualifier` 是别名或表名：补该表的列；
  - `qualifier` 是 schema：补该 schema 的表和视图；
  - 需要列且无限定：补语句中各表的列，列名后注明所属表；
  - 关键字按解析器给出的候选，不用整个关键字表。
- 元数据：
  - 复用对象树的 react-query 查询键，对象树已经取过的不重复取；
  - 表的列第一次需要时才取（`/tables` 明细）；
  - 跨 schema 的 `schema.` 按需取该 schema 的对象列表；
  - 对象树刷新时，补全用的缓存一并失效；
  - 元数据还在加载时先给出关键字和已有的对象，不等待。
- 标识符：补全插入的名称在需要时按方言加引号（MySQL 反引号；PG、Oracle、达梦双引号），规则与 M1b1 生成 SQL 时一致。

## 5. 语法检查的表现

- 标准档：红色波浪线，`MarkerSeverity.Error`，文字是解析器的错误信息。
- 兼容档：黄色波浪线，`MarkerSeverity.Warning`，文字后加"（按 MySQL / PostgreSQL / Oracle 语法检查，<变体名> 的扩展语法可能被误报）"。
- 错误文字跟随界面语言：`dt-sql-parser` 用 `zh_CN` / `en_US`；补充文字走前端词条。
- 与 M1b1 执行出错的标红相互独立，使用不同的标记来源（owner），不互相覆盖或清除。
- 设置 `ui.editor.syntax-check`（默认开，值为 `off` 时关闭；设置名只允许小写字母、数字和 `._-`）：关闭后不做语法检查，也不为检查加载解析器；补全照常可用。
- 不阻止执行。状态栏显示当前脚本的语法问题数，点击跳到第一处。

## 6. Oracle / 达梦技术验证（计划的第一个任务）

做法：用 grammars-v4 的 PL/SQL 语法和 `antlr-ng` 生成 TypeScript 解析器，接上 `antlr4-c3`，放进 worker 实测。

合格线（全部满足才算达标）：
1. 解析器打包后 gzip ≤ 3 MB，只在打开 Oracle / 达梦连接时加载；
2. 在 worker 里解析 1000 行典型脚本 ≤ 300 ms（开发机，首次解析之后）；
3. 补全样例（`SELECT … FROM |`、`别名.|`、`WHERE |`、`INSERT INTO t(|`、`PACKAGE BODY` 内部的语句等）中，期望的对象类型命中 ≥ 80%；
4. 达梦：用测试库中的真实 DDL 和查询统计误报，结果记入说明；误报多不影响结论（达梦属兼容档）。

结果：
- 达标：生成脚本提交进仓库；生成的代码不提交，构建前自动生成（`antlr-ng` 不需要 Java，约 5 秒，输入不变时跳过）。Oracle、达梦走完整流程。
- 不达标：验证代码不保留；Oracle、达梦用轻量补全——解析器关键字表之外，以简单词法识别 `FROM` / `JOIN` 后的表和别名，补全对象与列；不做语法检查。结论与测量数据写进 `docs/notes`。

## 7. 错误处理

- worker 加载解析器失败或崩溃：本次编辑会话中该方言退回轻量补全，不再检查语法，控制台记录一次，不打扰用户。
- 解析超时（单次 > 2 s）：丢弃本次结果；连续三次超时则该编辑器停止语法检查，状态栏提示。
- 元数据请求失败：补全中略过该部分，不弹错误。

## 8. 测试

- 单元测试（Vitest）：`dialects`、`completion`、`metadata`、`diagnostics` 的纯逻辑。
- 解析器测试：在 Node 环境直接调用 `parsers.ts`，每种方言一组补全和报错样例，包括中英文错误文字。
- 词条：新增的前端词条通过现有的中英一致性测试。
- 实测：5 个实库，中英两种界面，检查补全、红色和黄色标记、设置开关、懒加载（未用到的方言不下载解析器）。

## 9. 交付

- 依赖复查的结果（按 3.1 节），作为第一个任务单独提交。

- `docs/notes/m1b2-completion.md`：结构、方言对应、Oracle 验证的数据与结论、已知误报。
- M1 设计文档中的进度改为 "M1b2 已完成"。

## 10. 写计划时的调整（2026-10-26）

技术验证在写计划时完成，全部达标（数据见 `docs/notes/m1b2-completion.md`）。据此和实现中的发现，调整如下（上文已同步修改）：
- 生成的 PL/SQL 解析器不提交（生成工具是纯 TypeScript，生成快）。
- PL/SQL 先分句再逐句解析：ANTLR 遇到第一处错误后不再检查后面的语句，分句后每句单独报错，补全也只解析一句。
- 达梦语法补丁 11 处（`DROP … IF EXISTS`、`IDENTITY(m, n)`、`BIGINT` 等类型等），测试夹具中的达梦误报从 25/51 降到 2/51。
- 所有方言共用一个 worker；vite 的 worker 输出改为 ES 模块，解析器才能各自成块。
- 元数据补全最多等 1.5 秒；执行成功的 DDL 后补全元数据失效（连同后端元数据缓存）。
- 设置项名为 `ui.editor.syntax-check`。
