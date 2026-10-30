# M6b SQL Server（表设计器、导入、文档导出、比对与模型、迁移）设计

日期：2026-10-29。上级设计：M6a 设计 `docs/superpowers/specs/2026-10-29-m6a-sqlserver-design.md`（驱动、方言、按库浏览）；
M3 表设计器 `2026-10-27-m3-table-designer-design.md`；M2b 导入 `2026-10-27-m2b-import-design.md`；
M4a 模型与比对、M4c 文档导出；M5a 结构迁移、M5b 数据迁移。

## 1. 目标与范围

M6a 让 SQL Server 能连、能看、能查，写入类功能对它置灰。M6b 把这些功能补齐，SQL Server 与其他库同等可用：
- 表设计器：新建表、改表（变更语句）、需要时重建表；
- 导入：CSV / Excel 导入到新表或已有表（追加、清空后导入、按主键更新），执行 SQL 文件；
- 文档导出：数据字典和设计文档；
- 模型与比对：从库反向生成模型、三层比对、同步脚本；
- 迁移：SQL Server 作为来源和目标的结构迁移和数据迁移。

不做：
- 索引的包含列（`INCLUDE`）和筛选条件的编辑（设计器只显示，与 M6a 相同）；
- 改列类型时自动删除再重建引用该列的索引和约束（由服务器报错、整体回滚）；
- 分区表、内存优化表、列存储索引、全文索引的设计；
- geometry、geography、hierarchyid、sql_variant、rowversion 列的迁移（迁移方案中标"需人工处理"，与现有的不支持类型相同）。

## 2. 已定的决策（2026-10-29 与用户确认）

| 问题 | 结论 |
|---|---|
| 拆分 | 五块功能作为一个里程碑：共用同一套方言层改动（变更语句、批量写入、`IDENTITY_INSERT`、脚本分隔） |
| 默认值约束的名字 | 执行时查：生成的语句从 `sys.default_constraints` 查出名字再删除；不改公共模型，脚本拿到别的库也能用 |
| 设计器建的默认值 | 显式命名 `DF_表名_列名` |
| 迁移的实库路线 | SQL Server 与 MySQL、PG、Oracle、达梦互迁（8 条），另加以 SQL Server 为目标的续传、跳过各 1 条 |

## 3. 表设计器

新文件 `dblens-dialect/src/design/sqlserver.rs`（与 `mysql.rs`、`postgres.rs`、`oracle.rs` 并列），`design_sql` 的 SQL Server 分支调用它。

### 3.1 变更语句

| 变更 | 生成 |
|---|---|
| 新建表 | M6a 的建表语句；默认值写成 `CONSTRAINT DF_表_列 DEFAULT …`；注释用 `sp_addextendedproperty`（表和列） |
| 表注释 | 已有时 `sp_updateextendedproperty`，没有时 `sp_addextendedproperty`，清空时 `sp_dropextendedproperty`；用 `IF EXISTS (SELECT 1 FROM sys.extended_properties …)` 判断 |
| 加列 | `ALTER TABLE t ADD c 类型 [CONSTRAINT DF_t_c DEFAULT …] NULL / NOT NULL`；有注释时接着加扩展属性 |
| 删列 | 先删该列的默认值约束（见 3.2），再 `ALTER TABLE t DROP COLUMN c` |
| 列改名 | `EXEC sp_rename N'架构.表.列', N'新名', 'COLUMN'` |
| 改列（类型、可空） | `ALTER TABLE t ALTER COLUMN c 新类型 NULL / NOT NULL` |
| 改默认值 | 先删旧的默认值约束（3.2），有新默认值时 `ALTER TABLE t ADD CONSTRAINT DF_t_c DEFAULT … FOR c` |
| 列注释 | 同表注释，属性挂在列上 |
| 主键、唯一、检查、外键 | `ALTER TABLE t ADD CONSTRAINT 名 …` / `ALTER TABLE t DROP CONSTRAINT 名` |
| 索引 | `CREATE [UNIQUE] [CLUSTERED \| NONCLUSTERED] INDEX 名 ON t (…)` / `DROP INDEX 名 ON t` |
| 索引改名 | `EXEC sp_rename N'架构.表.索引', N'新名', 'INDEX'` |

- 外键的 `ON DELETE` / `ON UPDATE` 取值按 M6a 的 `DesignCaps`（NO ACTION、CASCADE、SET NULL、SET DEFAULT）。
- 所有名字经方言的方括号引用；`sp_rename`、扩展属性的参数是字符串字面量（`N'…'`）。

### 3.2 默认值约束

SQL Server 的默认值是有名字的约束，库自动起的名字带随机后缀（`DF__emp__name__3A81B327`），同一张表在不同库里名字不同。删除默认值、删列、改默认值时生成一段在执行时查名字的 T-SQL：

```sql
DECLARE @df sysname = (SELECT dc.name FROM sys.default_constraints dc
  JOIN sys.columns c ON c.object_id = dc.parent_object_id AND c.column_id = dc.parent_column_id
  WHERE dc.parent_object_id = OBJECT_ID(N'[架构].[表]') AND c.name = N'列');
IF @df IS NOT NULL EXEC (N'ALTER TABLE [架构].[表] DROP CONSTRAINT ' + QUOTENAME(@df));
```

这段作为一条语句执行（一个批），预览中原样显示。

### 3.3 事务与重建

- SQL Server 的 DDL 可以回滚：`design_transactional` 对 SQL Server 为真，全部语句在一个事务中执行，任一条失败整体回滚（与 PG 相同）。
- 需要重建的改动（与 M6a 已有的 `design_unsupported` 一致）：调整列顺序、改变自增属性、改生成列（计算列）。走现有的重建流程：
  建新表 → `SET IDENTITY_INSERT 新表 ON` → 复制数据 → `OFF` → 删旧表 → `sp_rename` 新表为原名 → 重建约束、索引、触发器和注释。复制也在同一事务中。
- 重建脚本中的存储子句：SQL Server 写成 `ON [文件组]`（不是 `TABLESPACE`）；读不到文件组时省略。
- 有依赖视图时与其他库相同：提示后照常重建（SQL Server 的视图按名字引用表，重建后仍可用，带 `SCHEMABINDING` 的视图会让删表失败并回滚）。

## 4. 批量写入（导入与数据迁移共用）

- 参数上限：SQL Server 一次请求最多 2100 个参数。导入语句生成器每条语句的参数上限改为由方言给出：SQL Server 2000，其余仍是 30000；每条语句至多 500 行不变。
- 按主键更新（导入的 `Upsert`）：一批生成一条多行 `MERGE`：
  ```sql
  MERGE INTO t USING (VALUES (@P1, @P2), (@P3, @P4)) AS s (a, b) ON t.id = s.id
  WHEN MATCHED THEN UPDATE SET t.b = s.b
  WHEN NOT MATCHED THEN INSERT (a, b) VALUES (s.a, s.b);
  ```
  结尾的分号必需；只映射了主键列时省略 `WHEN MATCHED`。
- 写入自增列：数据迁移已在 M6a 用 `SET IDENTITY_INSERT … ON/OFF`，写入显式值后自增计数自动推进，不需要调整。导入在映射了自增列时同样包上
  `IDENTITY_INSERT`（同一会话中先 ON、写完 OFF，出错时也关掉）；达梦用同一个函数，一并修正。
- SQL Server 作为来源：按主键分批读取沿用现有的增量读取，分页写成 `ORDER BY … OFFSET 0 ROWS FETCH NEXT n ROWS ONLY`；`money` 转精确小数，`uniqueidentifier` 转 UUID 或字符串；
  不支持的类型见第 1 节。
- SQL Server 作为目标：带小数秒的日期时间映射为 `datetime2(n)`（不用按 1/300 秒取整的 `datetime`），字符串映射为 `nvarchar(n)`，超过 4000 字符为 `nvarchar(max)`，布尔为 `bit`。
- 校验：抽样比对沿用现有的规整（`datetime2` 的精度、`char` 的尾部空格），不另加特例。

## 5. 脚本、比对与模型、文档导出

### 5.1 脚本分隔

语句拼成脚本的地方有三处：比对的同步脚本（`compare/sync.rs` 的 `sync_script`）、迁移脚本（`migrate/script.rs` 的 `script_text`）、
设计器保存对话框的"复制 SQL / 在查询标签中打开"（前端 `design/summary.ts` 的 `scriptOf`）。后端两处改用一个新的方言函数，前端按驱动走同样的规则：
- SQL Server：每条语句后接单独一行 `GO`（`CREATE VIEW`、`CREATE TRIGGER`、`CREATE PROCEDURE` 必须是批中第一条，3.2 的语句用了变量）；
- 其他库不变（分号；Oracle 的 PL/SQL 块后接 `/`）。

在 DBLens 中直接执行不受影响：语句本来就逐条发送。

### 5.2 比对与模型

反向生成模型、三层比对、同步脚本沿用现有流程；同步语句由第 3 节的变更语句生成。M6a 已有的比对规整：忽略库自动起名的 `PK__`、`UQ__`、`FK__`、`CK__`、`DF__` 约束名，
去掉默认值外层的括号。实库测试确认同一个库反向生成的模型与库比对没有差异。

### 5.3 文档导出

数据字典读元数据，注释来自扩展属性（M6a 已读取）。取消拦截即可。

### 5.4 结构迁移

M6a 已有 SQL Server 的类型映射、序列与自增、`NEXT VALUE FOR` 默认值、名字长度（128）和大小写规则。取消拦截，按第 7 节的路线实测。

## 6. 取消拦截

- 后端：去掉 `Service::feature_profile`（改回直接取连接配置），删去词条 `sqlserver.later`。
- 前端：去掉 `supportsWrites` 及据此置灰的菜单项、提示和来源/目标列表的过滤。

## 7. 测试

- 第一个任务是依赖审查：全部直接依赖升到最新稳定版（发布满 7 天），全量测试，单独提交。
- 单元测试：各类变更的语句（默认值约束的查删、`sp_rename`、扩展属性）、重建脚本、2000 个参数的拆分、多行 `MERGE`、脚本的 `GO` 分隔、导入时自增列的包裹。
- 实库测试（测试虚拟机 192.168.139.106 上的 SQL Server 2022，串行，不加 `--include-ignored`）：
  - 设计器：加列、删列、改名、改类型、改默认值、注释、约束、索引；一次重建（调整列顺序、带自增列的表）。每次执行后核对结构和数据。
  - 导入：CSV 到新表、到已有表（追加、清空后导入、按主键更新、带自增列）。
  - 比对：从 SQL Server 反向生成模型，与库自身比对无差异；改动后生成同步脚本并执行。
  - 文档导出：SQL Server 库的数据字典。
  - 迁移：SQL Server ↔ MySQL、PG、Oracle、达梦 8 条路线（行数和抽样逐值比对一致，自增和序列接着往下走），以 SQL Server 为目标的续传、跳过各 1 条。
  - 回归：全部 13 个实库目标。
- 兼容：在 Win11 测试虚拟机（192.168.139.103）的 SQL Server 2025 Express 上再跑一遍 SQL Server 的实库测试（中文环境、中文报错）。
- 浏览器实测：设计器、导入、比对、文档导出、迁移各在 SQL Server 上走一遍，菜单不再置灰。

## 8. 说明文档

`docs/notes/m6b-sqlserver.md`：结构、与本设计不一致的地方、已知限制、验证情况。
