# M6b SQL Server：表设计器、导入、文档导出、比对与模型、迁移

依据：设计文档 `docs/superpowers/specs/2026-10-29-m6b-sqlserver-design.md`。

## 结构

```text
crates/dblens-dialect/src/design/sqlserver.rs  设计器的变更语句：带名字的默认值（DF_表_列）、执行时查名删除默认值约束、sp_rename、扩展属性注释
crates/dblens-dialect/src/design/sql.rs        SQL Server 分支、可回滚（design_transactional）、rename_table_sql
crates/dblens-dialect/src/design/rebuild.rs    重建：文件组 ON […]、IDENTITY_INSERT、sp_rename 改回原名并把默认值约束改回按原表名命名、sp_refreshview
crates/dblens-dialect/src/design/diff.rs       NONCLUSTERED 是默认的索引方法
crates/dblens-dialect/src/import.rs            每条语句的参数上限由方言给出（SQL Server 2000）、多行 MERGE、NULL 直接写进语句
crates/dblens-dialect/src/changes.rs           表数据编辑：NULL 直接写进语句
crates/dblens-dialect/src/variant.rs           Family::terminate：脚本中一条语句的写法（SQL Server 后接 GO）
crates/dblens-dialect/src/lib.rs               schema_switch_sql：SQL Server 不切换 schema
crates/dblens-dialect/src/compare/sync.rs      同步脚本的表改名用 rename_table_sql
crates/dblens-dialect/src/migrate/             序列（AS BIGINT、NO CACHE / NO CYCLE）、默认值（时间、UUID、布尔）、源表达式的方括号和 N''、
                                               NEXT VALUE FOR 带 schema、script_text 按目标结束语句
crates/dblens-service/src/service.rs           scoped_profile（替代 feature_profile，不再拦截）
crates/dblens-service/src/{design,import/job,compare,document/job,migrate/mod,model/ops}.rs  请求加 catalog
crates/dblens-service/src/import/job.rs        映射了自增列时包上 IDENTITY_INSERT
crates/dblens-service/src/migrate/values.rs    时间按目标精度截断后再写
crates/dblens-api/src/{design,tasks}.rs        读原表、建表语句预览的 catalog 参数
web/src/shared/api.ts                          withCatalog、planEnds：请求和保存的方案中拆开合成的 schema
web/src/shared/schemaOptions.ts                连接下可选的 schema（SQL Server 列出各用户库的架构）
web/src/features/{tree,data,compare,migrate,model,design,import,document}/  取消置灰、下拉框、GO、库.架构 的显示
```

## 与设计文档不一致的地方

- 库名的传递（设计文档第 6 节，写计划时补充）：前端界面中仍用合成的 schema（对象树、标签、比对标签的状态），只在请求后端和另存迁移方案时拆成
  `catalog` + `schema`；比对标签保存在本机的界面状态中，没有拆开。
- SQL Server 的反向生成模型：所选的架构必须在同一个库中（模型中的 schema 不带库），否则前端提示。
- schema 下拉框只列用户库和非系统架构（系统库 master、model、msdb、tempdb 不列）。
- 写入时的 NULL：驱动把类型未知的 NULL 参数按 nvarchar 传，写进 varbinary 等列会报"不能隐式转换"（M6a 的假设不对）。
  导入、数据迁移、表数据编辑对 SQL Server 改为把 NULL 直接写进语句，不占参数。
- 删除默认值约束的那段 T-SQL 用 `sp_executesql` 执行拼好的语句：`EXEC (…)` 的括号里只能拼字面量和变量，不能调用 `QUOTENAME`。
- 重建：新表上的默认值约束按临时表名命名（约束名在 schema 内不能重复），改回原名后再用 `sp_rename … 'OBJECT'` 改成按原表名命名，
  否则下次重建同一张表时重名。依赖本表的视图在重建后执行 `sp_refreshview`。
- 改列类型：实测 SQL Server 允许在有默认值约束、唯一约束的列上直接 `ALTER COLUMN`（加长），不必先删再建。
- 导入的按主键更新：自增列不能更新，不写进 `MERGE` 的 `UPDATE SET`。
- 迁移（实测发现后修正）：
  - 序列写成 `AS BIGINT … NO CACHE / NO CYCLE`，超出 BIGINT 的边界（如 Oracle 的 28 个 9）不写；
  - 默认值：布尔写 1、0；当前时间写 `SYSDATETIME()`、`SYSDATETIMEOFFSET()`、`CAST(GETDATE() AS DATE)`，UUID 写 `NEWID()`；
    SQL Server 作为来源时认得 `SYSDATETIME`、`SYSDATETIMEOFFSET`；
  - SQL Server 作为来源时，检查约束等表达式中的 `[列]`、`N'…'` 能转换；
  - 默认值 `NEXT VALUE FOR 序列` 补上 schema（SQL Server 不切换当前 schema）；
  - 时间值按目标列的精度截断后再写（所有目标）：达梦写入时四舍五入，与校验的截断不一致。
- 同步脚本的表改名统一由 `rename_table_sql` 生成：SQL Server 用 `sp_rename`；MySQL 的新名带 schema（原来不带，当前库不同时会把表移到别的库）。
- 比对和设计器把 `NONCLUSTERED` 视为默认的索引方法（与 BTREE、NORMAL 相同），否则同一个库反向生成的模型与库比对时每个索引都报差异。

## 已知限制

- 非 Unicode 的 `varchar`、`char` 列在默认排序规则下存不了中文（数据库的限制）；DBLens 生成的字符串列一律是 `nvarchar`。
- 改列类型时，若有索引、检查约束、计算列引用该列，由服务器报错、整体回滚；不自动删除再重建。
- 带 `SCHEMABINDING` 的视图引用的表不能重建（删表失败、整体回滚）。
- 触发器定义中不带 schema 的表名按执行者的默认架构解析（重建时原样执行触发器 DDL）。
- 索引的包含列、筛选条件不能在设计器中编辑；分区表、内存优化表、列存储索引、全文索引不支持设计。
- geometry、geography、hierarchyid、sql_variant、rowversion 列的迁移需人工处理（迁移方案中列为不支持的映射）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过（不连库 837 个）。按任务拆分的每个提交都单独跑过 fmt、clippy 和全部测试。
  - 方言层：SQL Server 的建表、全部变更、改列、重建快照；默认值约束名、计算列表达式；2000 个参数的拆分、多行 MERGE（NULL 直接写）；脚本的 GO；
    迁移的 PG → SQL Server 快照、布尔和时间默认值、来源表达式的方括号和 N''。
- 实库（串行）：
  - SQL Server 2022（测试虚拟机 192.168.139.106 上的 Docker）：设计器（加列、删列、改名、改类型、改默认值、注释、约束、索引；一次重建；中途失败整体回滚）、
    导入（追加、清空后导入、按主键更新、带自增列）、模型与比对（反向生成、改名和列变化的同步、写回模型）、文档导出、M6a 的全部测试。
  - 迁移：SQL Server ↔ MySQL、PG、Oracle、达梦 8 条路线和以 SQL Server 为目标的续传、跳过各 1 条（行数和抽样逐值比对一致，自增和序列接着往下走）。
  - 回归：原有 12 个实库目标加 SQL Server 2022 一起串行跑全部后端测试，813 个通过（约 6 分钟；达梦偶发的"网络通讯失败"重跑后通过）。
  - SQL Server 2025 Express（Win11 测试虚拟机 192.168.139.103，中文环境）：上面 SQL Server 的测试和 10 条迁移路线全部通过。
- 前端：tsc、Vitest 287 个（库名拆分、迁移方案、反向生成的库检查、设计器脚本的 GO）、打包通过。
- 浏览器：对象树中 SQL Server 的菜单不再置灰；设计器读原表（标题 `库.架构.表`）、加列的预览（一个事务）、保存执行后列已加上，再删掉；
  比对的 schema 下拉框列出 `dblens_test.dblens_meta`、`dblens_test.dbo`，同一个架构与自身比对没有差异；从对象树"迁移到…" MySQL，读取结构后列出表和需要处理的不支持映射。
- 未验证：桌面程序（Tauri）中的这些功能；文档导出和导入在浏览器中的完整操作（接口转换有单元测试，后端有实库测试）；SQL Server 2017。
