# M6a SQL Server（连接、浏览、查询）设计

日期：2026-10-29。上级设计：总设计文档 `docs/superpowers/specs/2026-10-22-dblens-design.md`（驱动、方言、元数据模板的分层）；
国产库第一批设计 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`（新增变体的做法）；
M1b2 设计 `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`（grammars-v4 语法、补全）。

## 1. 目标与范围

新增第五种协议 SQL Server：能连、能看、能查。

M6 拆成两段：
- **M6a（本文）**：驱动和连接（含 TLS、三种认证）、对象树（连接 → 数据库 → 架构 → 对象）、表结构查看、
  SQL 执行（按 `GO` 分批）和结果编辑、表数据分页浏览、运维查询、执行计划、危险语句确认、编辑器的 T-SQL 检查/补全/格式化、结果导出。
- **M6b（以后）**：表设计器和 DDL 生成、导入、文档导出、结构对比、结构和数据迁移（SQL Server 作为来源和目标）。

不做：
- Azure SQL 等云上版本的单独识别（按协议处理，见"协议优先"原则）；
- Kerberos 认证、Entra ID（Azure AD）认证；
- 共享内存、命名管道连接（只走 TCP）；
- geometry、geography、hierarchyid、sql_variant 的解析显示（按二进制或文本显示）；
- SQLCMD 模式的其他命令（`:setvar`、`:r` 等），只认 `GO`。

## 2. 已定的决策（2026-10-29 与用户确认）

| 问题 | 结论 |
|---|---|
| 拆分 | M6a 连接/浏览/查询，M6b 写入类功能 |
| 驱动 | tiberius 0.13（rustls）；微软的 mssql-tds 0.3 太新（0.x、接口仍在变），以后可在驱动 crate 内替换 |
| 认证 | SQL 账号、指定 Windows 账号（NTLM）、当前 Windows 用户（仅 Windows）；Windows 认证在用户的 Win11 虚拟机上实测 |
| 对象树 | 像 SSMS：连接 → 数据库 → 架构 → 对象，一个连接看到全部有权限的库 |
| 脚本执行 | 按单独一行的 `GO` 分批，每批整段发给服务器（与 SSMS、sqlcmd 一致） |
| 版本 | 支持 2017 及以上；日常实测 2022，2017、2025 各跑一轮 |

## 3. 驱动：`dblens-driver-sqlserver`

### 3.1 连接

- `DriverId::SqlServer`，URL 前缀 `sqlserver://` 或 `mssql://`，默认端口 1433。
- tiberius 走 tokio 的 TCP 流（`tokio-util` 的 compat 适配）。
- 实例名（选填，如 `SQLEXPRESS`）：填了就通过 SQL Browser（UDP 1434）查实际端口（tiberius 的 `sql-browser-tokio`）。
- 默认数据库（选填）：登录后的初始库；不填用登录账号的默认库。
- Azure 的重定向（`Error::Routing`）：按返回的主机和端口重连一次。

### 3.2 认证

| 方式 | tiberius | 平台 |
|---|---|---|
| SQL 账号 | `AuthMethod::sql_server(user, password)` | 全部 |
| 指定 Windows 账号 | `AuthMethod::windows("域或机器名\\用户", password)`（NTLM） | 全部 |
| 当前 Windows 用户 | `AuthMethod::Integrated` | 仅 Windows |

- 浏览器访问模式下，"当前 Windows 用户"是服务端进程的身份，不是浏览器所在电脑的身份；表单上注明。
- 非 Windows 平台选了"当前 Windows 用户"时报错说明，不静默改用别的方式。

### 3.3 加密

| DBLens 的 TLS 选项 | tiberius |
|---|---|
| 关闭 | `EncryptionLevel::NotSupported`（服务器强制加密时由服务器报错） |
| 优先 | `EncryptionLevel::On`，不校验证书 |
| 必须 | `EncryptionLevel::Required`，不校验证书 |
| 校验证书 | `EncryptionLevel::Required` + 校验证书（含主机名） |

- 另有"信任服务器证书"开关，等同于 SSMS 的同名选项：打开时"校验证书"也不校验。
- SQL Server 2022 默认强制加密、证书自签名，所以"必须"不校验证书，否则默认安装连不上。

### 3.4 执行与结果

- `execute` 收到的是一整批；一批里有几条语句就返回几个结果集或影响行数（`multiple_result_sets = true`）。
- 参数：`@P1`、`@P2`……（tiberius 的写法），方言的 `placeholder` 生成这种形式。
- 事务：自动提交时不显式开事务；手动提交时执行 `BEGIN TRANSACTION`，`commit` / `rollback` 执行对应语句；
  `@@TRANCOUNT` 用于判断是否有未提交的事务。
- 服务器消息（`PRINT`、`RAISERROR` 低级别）：按顺序显示在该次执行的消息里（与影响行数等消息放在一起）。
- 报错：错误号、级别、状态、过程名、批内行号都保留；行号换算成编辑器里的行号（批的起始行 + 批内行号 − 1）。
- 大字段：按现有的 `set_lob_limit` 截断为预览。

### 3.5 类型映射

| SQL Server | DBLens 的值 |
|---|---|
| bit | 布尔 |
| tinyint、smallint、int、bigint | 整数 |
| decimal、numeric、money、smallmoney | 精确小数 |
| float、real | 浮点 |
| date、time、datetime、datetime2、smalldatetime | 日期 / 时间 / 日期时间 |
| datetimeoffset | 带时区的日期时间 |
| char、varchar、nchar、nvarchar、text、ntext、xml | 文本（大字段截断为预览） |
| binary、varbinary、image、rowversion / timestamp | 二进制 |
| uniqueidentifier | UUID |
| geometry、geography、hierarchyid、sql_variant 及其他 UDT | 二进制或文本，不解析 |

### 3.6 取消（需先验证）

TDS 协议支持取消（attention 包）。计划的第一项实现任务先做小实验，确认 tiberius 0.13 能否在执行中发送取消：
- 能：`cancel_handle` 发送取消，连接保留；
- 不能：取消改为关闭这个连接的套接字并自动重连，服务器发现断开会终止执行；会话状态（`USE` 过的库、临时表、
  未提交的事务）会丢失，界面提示。实验结论写进说明文档。

## 4. 方言

### 4.1 家族与变体

- `Family::SqlServer`、`Variant::SqlServer`，显示名 "SQL Server"。
- 标识符：方括号引用，`]` 写成 `]]`；`qualified` 支持 `[库].[架构].[名]`。
- 字面量：字符串一律 `N'…'`（Unicode，写入非 Unicode 列时由服务器转换），二进制 `0x…`，布尔写成 `1`/`0`，
  日期时间写成 ISO 8601 字符串（如 `'2026-10-29T12:00:00.000'`，不受 `DATEFORMAT` 设置影响）。
- 插入只有默认值的行：`INSERT INTO t DEFAULT VALUES`。

### 4.2 `GO` 分批

后端拆分器和前端编辑器用同一套规则：
- `GO` 必须单独占一行（前后可有空白），不区分大小写；可以带次数 `GO 5`（该批执行 5 次）；行尾可以有 `--` 注释。
- 字符串、方括号标识符、`/* */` 注释（可嵌套）里的 `GO` 不算。
- 没有 `GO` 的脚本整体是一批。
- 每批保留在原文中的起始行号，用于错误定位。
- 选中文本执行时，选中的部分也按 `GO` 切分。
- "执行光标处"执行光标所在的批。

### 4.3 分页

- 浏览表数据：`ORDER BY 主键 OFFSET n ROWS FETCH NEXT m ROWS ONLY`；没有主键时 `ORDER BY (SELECT NULL)`，
  界面提示翻页时行的顺序不保证稳定。
- 用户写的查询不改写，流式读取到一页即停（与其他库相同）。

### 4.4 结果编辑

- 沿用现有的增删改生成；新增行时跳过自增列（identity）、计算列、rowversion 列。
- 没有主键和唯一键的表只读。

## 5. 树、元数据、会话

### 5.1 数据库层

- 启用 `Capabilities::catalogs`。新的元数据项 `MetaKey::Catalogs`：
  `sys.databases` 中 `state = 0`（在线）且 `HAS_DBACCESS(name) = 1` 的库。
- 系统库（master、model、msdb、tempdb）排在后面，标为系统库。
- 树：连接 → 数据库 → 架构 → 对象分组。其他驱动 `catalogs = false`，树结构不变。
- 元数据查询的参数 `MetaArgs` 增加 `catalog`；模板新增 `{{ catalog() }}`，输出按标识符规则加方括号转义的库名，
  用于 `{{ catalog() }}.sys.tables` 这类三段名。库名不经绑定参数，但只能经这个函数进入 SQL，不拼原始字符串。

### 5.2 元数据项

模板文件 `sql/sqlserver.sql`（家族基础文件），运维查询 `sql/ops/sqlserver.sql`。

| 元数据项 | 来源 |
|---|---|
| Catalogs | `sys.databases` |
| Schemas | `[库].sys.schemas`（排除 `sys`、`INFORMATION_SCHEMA`、`guest` 以及 `db_*` 角色架构） |
| Tables、Views | `[库].sys.tables`、`[库].sys.views` |
| Sequences | `[库].sys.sequences` |
| Routines | `[库].sys.procedures`、`[库].sys.objects`（FN、IF、TF） |
| Synonyms | `[库].sys.synonyms` |
| Columns | `[库].sys.columns`、`sys.types`、`sys.default_constraints`、`sys.computed_columns`、`sys.identity_columns` |
| Keys | `[库].sys.key_constraints`、`sys.foreign_keys`、`sys.foreign_key_columns` |
| Checks | `[库].sys.check_constraints` |
| Indexes | `[库].sys.indexes`、`sys.index_columns`（含包含列、筛选条件） |
| Triggers | `[库].sys.triggers` |
| ViewDefinition、RoutineSource、TriggerDdl | `[库].sys.sql_modules` |
| ReservedWords | 固定清单 |
| Ddl | 由元数据生成（现有的按元数据拼 DDL 的代码加 SQL Server 分支） |
| ReferencingKeys、DependentViews | `[库].sys.foreign_keys`、`sys.sql_expression_dependencies` |
| Grants | `[库].sys.database_permissions` |
| TableRows、TableStorage | `[库].sys.dm_db_partition_stats` |

没有 `VariantProbe`（不识别兼容产品）。

### 5.3 会话

- 编辑器标签选的库与连接当前的库不同时，执行前先执行 `USE [库]`；当前库用 `DB_NAME()` 读取。
- SQL Server 不能在会话中切换默认架构，不带架构的名称按登录用户的默认架构解析；标签上选的架构只影响补全。
  DBLens 生成的 SQL 一律带架构名。

## 6. 运维与执行计划

| 功能 | 来源 |
|---|---|
| 会话列表 | `sys.dm_exec_sessions` 关联 `sys.dm_exec_requests`、`sys.dm_exec_sql_text`（当前 SQL） |
| 锁 | `sys.dm_tran_locks` |
| 终止会话 | `KILL <会话号>` |
| 服务器参数 | `sys.configurations` |

- 这些视图需要 `VIEW SERVER STATE` 权限；没有时显示数据库返回的错误并提示所缺权限。
- 执行计划：`SET SHOWPLAN_ALL ON`、执行该批、`SET SHOWPLAN_ALL OFF`，取估算计划的表格结果，用现有结果网格显示；
  语句不会真正执行。

## 7. 危险语句确认

生产环境的连接上：
- 用 sqlparser 的 `MsSqlDialect` 把每批拆成语句逐条分类：不带 WHERE 的 UPDATE / DELETE、DROP、TRUNCATE 要求确认（与其他库相同）。
- 解析不了的批（`IF`、`WHILE`、`BEGIN…END` 等过程式代码）：按关键字扫描（跳过字符串和注释），
  出现 DROP、TRUNCATE、DELETE、UPDATE、MERGE 即整批要求确认。
- SQL Server 特有、一律要求确认：`KILL`、`SHUTDOWN`、`DBCC`、`ALTER DATABASE`、`RESTORE`。

## 8. 编辑器

- 语法：grammars-v4 的 `sql/tsql`（按依赖原则固定到发布满 7 天的提交），放在 `web/grammars/tsql/`，
  生成脚本与 PL/SQL 相同（`web/scripts/gen-tsql.mjs`，输出到 `src/features/sql/tsql/gen/`，不提交），按需加载。
- SQL Server 按标准档检查（语法错误标红）。
- 补全：antlr4-c3；表、列、库、架构；三段名 `库.架构.表` 按需取别的库的元数据。
- 格式化：sql-formatter 的 `transactsql`。
- 标识符引用：方括号。
- `GO`：编辑器先按 4.2 节切批，再把每批交给解析器；`GO` 行不参与语法检查。
- 函数清单：常用内置函数（字符串、日期、数学、转换、聚合、窗口、JSON 函数）供补全和高亮。

## 9. 连接表单与界面

- 产品列表加 SQL Server，图标取 simple-icons（没有则自绘）。
- 认证方式：SQL 账号 / Windows 账号 / 当前 Windows 用户（只在 Windows 上显示；浏览器访问模式下注明身份来源）。
- 字段：主机、端口、实例名（选填）、默认数据库（选填）、用户名、密码、TLS 选项、信任服务器证书。
- 密码沿用现有的加密保存。
- M6b 的功能（表设计器、导入、文档导出、结构对比、迁移）：对 SQL Server 连接入口置灰，悬停提示"SQL Server 将在后续版本支持"；
  后端对应接口返回"暂不支持"的错误；迁移的来源和目标列表不出现 SQL Server。

## 10. 测试

- 单元测试：`GO` 切分（字符串、注释、方括号里的 GO，`GO 5`，行尾注释，行号）、类型转换、标识符和字面量、
  分页、危险语句分类、元数据模板快照（含 `catalog()` 的转义）。
- 驱动一致性测试（`dblens-driver` 的 conformance）：类型往返、多结果集、事务、取消、大字段截断。
- 实库测试：`DBLENS_TEST_SQLSERVER_URL`，串行运行（`--test-threads=1`，不加 `--include-ignored`）。
  日常用测试虚拟机上 Docker 的 SQL Server 2022；2017、2025 各跑一轮。
- Windows 认证：`DBLENS_TEST_SQLSERVER_WINAUTH_URL`（指定账号，宿主机到 Win11 虚拟机）和
  `DBLENS_TEST_SQLSERVER_INTEGRATED_URL`（当前用户，在 Win11 虚拟机上运行），没设置就跳过。
- 浏览器验证：连接、树（多个库）、执行带 `GO` 的脚本、多结果集、PRINT 消息、错误行号、编辑结果、运维页面、执行计划。
- 第一个任务是依赖审查：全部直接依赖升到最新稳定版（发布满 7 天），全量测试，单独提交。

## 11. 测试环境

- Linux 测试虚拟机（192.168.139.106）：Docker 跑 `mcr.microsoft.com/mssql/server:2022-latest`（约 2G 内存），
  2017、2025 的镜像只在兼容轮次临时启动。sa 密码只在虚拟机的 `creds.env` 中。
- Win11 虚拟机（用户准备）：SQL Server 2022 Developer、混合认证、TCP 1433 及防火墙放行、OpenSSH 服务器、一个本地 Windows 账号。

## 12. 说明文档

`docs/notes/m6a-sqlserver.md`：结构、与本设计不一致的地方、取消实验的结论、已知限制、验证情况。
