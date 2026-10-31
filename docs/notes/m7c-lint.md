# M7c 规范检查收尾

依据：设计文档 `docs/superpowers/specs/2026-10-30-m7-lint-design.md` 第 2 节。

## 结构

```text
crates/dblens-lint/src/rules/naming.rs        name.pattern：名字必须匹配或不得匹配正则
crates/dblens-lint/src/rules/mod.rs           check_params：保存规范时校验正则；Ctx::storage
crates/dblens-lint/src/rules/{table,object}.rs  行数、自增步长、ON UPDATE、字符集、引擎改为真实判断
crates/dblens-lint/src/engine.rs              check_table（设计器用）；LintInput.storage
crates/dblens-dialect/sql/*.sql               [lint_tables]：整个 schema 的存储信息，只读系统表
crates/dblens-dialect/src/meta/assemble.rs    LintTable、lint_tables
crates/dblens-service/src/lint/job.rs         lint_table；检查时取存储信息；记录保存来源
crates/dblens-api/src/lint.rs                 POST /api/lint/table
web/src/features/lint/RunsView.tsx            检查记录
web/src/features/lint/DiffView.tsx            两次结果对比，计算在 logic.ts 的 diffRuns
web/src/features/lint/RunView.tsx             点问题打开表设计器
web/src/features/lint/LibraryView.tsx         加规则条款（名称正则、必备字段）
web/src/features/design/DesignLintPane.tsx    设计器“规范”子标签
```

## 接口

```text
POST /api/lint/table   {ruleset, source, table}，只返回未通过的条款，不落盘
```

检查记录多了 `source`、`model_path`。旧记录没有，问题不可点击。

## 与设计文档不一致、写实现时定的

- 规范没有 `custom:` 字段：自定义规则就是挂 `name.pattern`、`table.required_columns` 的条款。
- `model_path` 单独存：设计器按模型目录打开，模型编号只在本次打开时有效。
- 内置规范（数洞通用、事务型严格）新增 `db.charset`、`db.engine` 两条条款，只对 MySQL 系生效，规范条款数各加 2。
- `db.engine` 默认允许 `InnoDB`、`oceanbase`：OceanBase 的 MySQL 模式报告的引擎是 `oceanbase`。
- 行数规则默认仍关闭。行数是统计信息估计，PostgreSQL 没分析过的表取不到。
- 自增步长取服务器设置 `@@auto_increment_increment`，不是单表属性。
- 存储信息查询失败不影响检查，相关规则报无法判断。

## 已验证

- 后端 1013 项、前端 339 项、clippy、格式检查（不含 `dblens-desktop` 和 `dblens-server`）。
- 真实鼠标（模型来源）：检查记录列表、勾选两次对比、点问题打开表设计器、“规范”子标签和角标、改表注释后角标从 5 降到 4、加规则条款、错误正则被拒绝、正确的正则能保存。

- 实库（2026-10-31）：`lint_tables_query_runs_on_every_database` 在 MySQL 9.7、MariaDB 12.3、PostgreSQL 18、Oracle 26ai、达梦 8、SQL Server 2022、TiDB、OceanBase、openGauss、金仓、GBase 8s 上逐库通过；`mysql_lint_reads_storage_facts` 确认字符集、引擎、`ON UPDATE`、自增步长、行数都能判出问题。
- 真实鼠标（连接来源，MySQL）：检查记录打开结果、“无法判断”为 0、点问题打开真实表的设计器、“规范”子标签带角标。
- 真实鼠标（浏览器模式）：用自定义规范发起检查，不符合正则的表判为未通过，符合的不报。

- 桌面 dev 窗口（WebView2 调试端口发真实输入并截图，连真实达梦 `CCTAR2`，76 张表、809 个问题）：发起检查、记录列表、对比、点问题打开设计器、“规范”子标签（角标 17）、Word、Excel、自查表下载到 Downloads。

走查发现并修复：空 schema 显示“通过率 100%”会误导，记录增加 `table_count`，为 0 时结果页给提示、不显示通过率，报告写表数量；两位数角标压住“规范”二字。

## 待验证（真实运行）

- 用 Word、Excel 实际打开导出的文件，只解压核对过内容。
