# M6a SQL Server：连接、浏览、查询

依据：设计文档 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`。

## 结构

```text
crates/dblens-driver-sqlserver/       新驱动（tiberius 0.13）
  src/lib.rs     URL 解析成 tiberius 的 Config（认证、实例名、TLS、信任服务器证书）、连接（Azure 重定向重连一次）
  src/actor.rs   后台任务独占连接，命令走 mpsc，结果走有界通道（256）；取消用 Notify；
                 无参数时整批 simple_query（USE、临时表留在会话里），有参数时 sp_executesql
  src/conn.rs    Connection 实现：手动提交时 BEGIN TRANSACTION，提交/回滚前看 @@TRANCOUNT
  src/exec.rs    执行流：连续的行合并成一批
  src/types.rs   列类型和值的转换
  src/error.rs   错误号、批内行号换算成位置
vendor/tiberius/                      tiberius 0.13.0 的补丁（见 vendor/PATCHES.md）：
                                      QueryItem::Done / Info（影响行数和 PRINT 消息不丢）、取消后丢弃到 attention 确认为止
crates/dblens-dialect/
  src/variant.rs ident.rs literal.rs  SqlServer 家族：方括号、@P1 占位符、N'' 字符串、0x 二进制、OFFSET FETCH
  src/split.rs   GO 分批（单独一行、可带次数和行尾注释；字符串、方括号、嵌套注释里的不算；次数上限 1000）
  src/ddl.rs types.rs design/catalog.rs migrate/*  DDL 生成、类型映射；表设计器和迁移的入口报"暂不支持"
  sql/sqlserver.sql  元数据模板（含 catalogs）；sql/ops/sqlserver.sql 运维查询
crates/dblens-service/
  src/profile.rs   in_catalog、scope（连接 id + 库名）
  src/metadata.rs  元数据池和缓存按 scope 区分；catalogs()
  src/policy.rs    classify_batch（危险语句）
  src/service.rs   feature_profile：M6b 的功能对 SQL Server 报 sqlserver.later
  src/session.rs   执行计划的收尾语句（SET SHOWPLAN_ALL OFF）
crates/dblens-api/src/routes.rs  元数据接口加 ?catalog=，GET /profiles/{id}/catalogs
web/grammars/tsql/                    grammars-v4 的 T-SQL 语法（固定提交，MIT）
web/scripts/gen-tsql.mjs              生成解析器到 src/features/sql/tsql/gen/（不提交）
web/src/features/sql/tsql/parser.ts   GO 分批、语法检查、补全（三段名、别名）
web/src/features/tree/                库 → 架构 → 对象
web/src/shared/api.ts                 库名和架构合成一个字符串（joinCatalog / splitCatalog / schemaLabel）、supportsWrites
web/src/features/connections/         认证方式、实例名、信任服务器证书
```

## 与设计文档不一致的地方

- 按库取元数据不用模板函数 `{{ catalog() }}` 拼三段名，而是元数据连接池按库区分，连接时指定库（`in_catalog`）；
  模板里只写 `sys.tables` 这类当前库的视图，库名不进入 SQL 文本。
- TLS 只有三档：关闭（`EncryptionLevel::Off`，登录报文仍加密）、必须（`Required`，不校验证书）、校验证书
  （`Required`，打开"信任服务器证书"时不校验，否则用指定的 CA）。DBLens 的 TLS 选项本来就没有"优先"。
- Windows 认证（指定账号和当前用户）只在 Windows 上可用：tiberius 在其他平台走 sspi-rs，其依赖 sha1 只有预发布版，
  不符合依赖原则。其他平台选了会报 `sqlserver.windows_auth_only`。表单在所有平台都显示这两项并加提示。
- 过程式代码等解析不了的批，按关键字扫描时只看 DROP、TRUNCATE、DELETE、UPDATE 和 SQL Server 特有的几个，不含 MERGE
  （与其他库一致，MERGE 按 DML 处理）。没有分号分隔的多条语句 sqlparser 也解析不了，同样走关键字扫描，宁可多问。
- 运维页面与其他库相同（会话、运行中、锁、存储、大表、慢 SQL），没有单独的"服务器参数"查询。
- 浏览表数据与其他库相同：执行 SELECT 后流式分批读取，不生成 `OFFSET … FETCH` 分页。
- 索引的包含列和筛选条件在元数据里列出，但生成 DDL 时不写（M6b 的表设计器再补）。
- 取消：tiberius 0.13 能发 attention 包，但原有的清理按令牌读，遇到被中断的结果集会读乱，下一条查询拿到空结果。
  补丁改为按包丢弃，直到收到带 attention 标志的 DONE 为止。取消后连接保留，`USE` 过的库、临时表、未提交的事务都还在。

## 已知限制

- Windows 认证只在 Windows 上可用（见上）。
- M6b 的功能（表设计器、导入、文档导出、结构对比、迁移、模型）对 SQL Server 置灰，后端报"暂不支持"。
- geometry、geography、hierarchyid、sql_variant 等类型不解析，按二进制或文本显示。
- 编辑器的标签选的架构只影响补全；不带架构的名称按登录用户的默认架构解析（SQL Server 不能在会话中切换默认架构）。
- 刷新只带架构名，同一连接中其他库的同名架构的缓存也一并失效（多刷新，不会少刷新）。
- 服务器的消息语言随登录名的默认语言（中文 Windows 上装的实例默认是简体中文）。语法错误的位置按错误号（102、156）取引号里的原文，不依赖语言；其他错误指向出错的行首。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下通过。
  - 方言层：`GO` 分批（字符串、注释、方括号里的 GO，`GO n`、次数上限、行尾注释、行号）、标识符和字面量、类型往返、DDL、危险语句分类、元数据模板快照。
  - 驱动：错误行号换算成位置（含中文消息）。
- 实库（串行）：
  - SQL Server 2022（测试虚拟机 192.168.139.106 上的 Docker）：驱动一致性 17 个（多结果集、影响行数与 PRINT、批内错误位置、读一半丢弃后连接可用、取消、批里自己提交、大字段）、元数据、服务（流程、工作台和执行计划、取消、对象、运维、导出、库和分批与 M6b 拦截、SQL 文件、编辑）、接口。
  - SQL Server 2025 Express（Win11 测试虚拟机 192.168.139.103，中文系统）：上面的 SQL Server 测试全部通过；指定 Windows 账号（NTLM，从开发机连）和当前 Windows 用户（在虚拟机上运行）的登录。
  - 回归：原有 12 个实库目标加 SQL Server 2022 一起串行跑全部后端测试，796 个通过（约 9 分钟；达梦偶发的"网络通讯失败"重跑后通过，OceanBase 要等虚拟机开机后初始化完）。
  - SQL Server 2017 没有实测（镜像下载太慢）。
- 前端：tsc、Vitest 286 个（T-SQL 的分批、检查、补全，库节点，合成的 schema，ER 图的外键）、打包通过。
- 浏览器（生产环境的 SQL Server 2022 连接）：库树（系统库标注）；带 PRINT、`GO`、`GO 2` 和错误语句的脚本（4 个结果、消息、错误行）；表数据中文、编辑提交（`N''`、原值条件、生产确认）；不带 WHERE 的 UPDATE 确认；执行计划及之后正常执行；架构和表的右键菜单（M6b 的项置灰）；DDL；生成 INSERT（跳过计算列）；ER 图的外键连线；运维的会话列表；新建连接的认证方式和 TLS 页。
  浏览器实测中发现并修正：驱动名序列化不一致导致界面新建连接被拒、右键菜单崩溃、DDL 和 ER 标题显示内部分隔符、ER 图的外键被当成图外引用。
- 未验证：桌面程序（Tauri）中的 SQL Server 连接；命名实例经 SQL Browser 查端口（测试实例固定了 1433）；Azure SQL 的重定向。
