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
