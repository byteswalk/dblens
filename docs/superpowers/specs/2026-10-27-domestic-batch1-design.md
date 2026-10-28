# 国产库适配第一批 设计

日期：2026-10-27。上级设计：总设计文档 `docs/superpowers/specs/2026-10-22-dblens-design.md` 第 3 节（里程碑）与进度记录（2026-10-27 用户决定：国产库适配作为 M3 之后的单独里程碑，分三批）。

## 1. 目标与范围

第一批是有 Docker 镜像或免费版、能实测的四个库：

| 库 | 驱动 | 兼容模式 |
|---|---|---|
| TiDB | MySQL | — |
| OceanBase 社区版（MySQL 模式） | MySQL | — |
| openGauss | PostgreSQL | A（Oracle 兼容，openGauss 默认）、PG |
| 人大金仓 KingbaseES V9R2 | PostgreSQL | Oracle（默认）、PG |

这四个库与 MySQL、PostgreSQL 支持同样的全部功能：连接、对象树、查询（拆分、出错位置、执行计划）、运维查询、数据编辑、导入导出、表设计器。

不做：
- 人大金仓的 MySQL、SQL Server 兼容版（V9R3、V9R4 是另外的产品线）；
- openGauss 的 B、C 模式；
- OceanBase 的 Oracle 模式（第三批，需要新驱动）；
- 国密 SM3 认证（GaussDB 可能用到，留给第二批）；
- 跨库的 SQL 转换。

第二批（只有云服务的 PolarDB、TDSQL、GaussDB）、第三批（GBase 8s、OceanBase 的 Oracle 模式）各自另开。

## 2. 已定的决策（2026-10-27 与用户确认）

| 问题 | 结论 |
|---|---|
| 测试环境 | 部署在测试虚拟机 192.168.139.106 上（见第 9 节），用实库测试驱动设计 |
| 支持深度 | 全部功能，与 MySQL、PostgreSQL 相同 |
| 兼容模式 | 人大金仓和 openGauss 的 Oracle 兼容模式与 PG 模式都覆盖 |
| 新建连接 | 按产品选，带默认端口和提示；连上后仍自动识别，以识别结果为准 |
| 整体方案 | 留在现有家族中，`Dialect` 增加兼容模式；SHA256 认证通过给 `tokio-postgres` 打补丁实现 |

## 3. 实测现状（2026-10-27）

把现有的实库测试指向新库（MySQL 测试改连 TiDB、OceanBase，PG 测试改连 openGauss PG 模式库、人大金仓 Oracle 模式）：

| 库 | 驱动一致性 | 元数据 | 服务层 | 主要问题 |
|---|---|---|---|---|
| TiDB | 20/20 | 4/5 | 43/47 | 没有 `sys.innodb_lock_waits`；出错位置格式不同；执行 SQL 文件时一条语句报 "Unsupported type"；读回索引的方法与草稿有差异 |
| OceanBase | 20/20 | 4/5 | 44/47 | 没有 `performance_schema` 中锁和慢 SQL 的表；出错位置、执行计划格式不同；读回索引差异同 TiDB |
| openGauss | 10/10 | 4/5 | 41/47 | 不支持 `GENERATED ... AS IDENTITY`；没有 `pg_get_partkeydef`；`pg_stat_activity` 缺 `backend_type`、`wait_event_type` 等列 |
| 人大金仓 | 9/10 | 4/5 | 43/47 | 版本解析成 9.2，元数据走了老分支（`proisagg`）；Oracle 模式不认 `'NaN'::numeric`；自增列被写成 NULL；开发版授权不支持物化视图 |

另外：
- 测试中写死"这是 MySQL / 这是 PG"的断言，以及接口层测试强制 TLS，在新库上失败。这两类属于测试本身的问题。
- 密码只存 SHA256 的 openGauss 账号，现有驱动连接时报 "error parsing response from server"。同时存 SHA256 和 MD5 的账号能连上，原因是服务器退回了 MD5 认证。

## 4. 产品识别与连接

### 4.1 兼容模式

`Dialect` 增加字段 `compat: Compat`，取值 `Native` 与 `Oracle`：

```rust
pub enum Compat { Native, Oracle }
```

- 只有人大金仓和 openGauss 可能是 `Oracle`，其他变体恒为 `Native`。
- 连上后沿用 `MetaKey::VariantProbe` 加 `Dialect::refine` 的机制探测：
  - 人大金仓：`show database_mode`，`oracle` → `Oracle`，`pg` → `Native`；
  - openGauss：`select datcompatibility from pg_database where datname = current_database()`，`A` → `Oracle`，`PG` → `Native`。
- 其他取值（openGauss 的 B、C，人大金仓的 mysql、sqlserver）按 `Native` 处理，连接信息中提示"未适配的兼容模式：<值>"。
- PG 协议一条连接对应一个库，openGauss 按库定的模式跟随连接，不需要额外处理。
- `Dialect` 上增加 `compat()` 和 `oracle_mode()`；需要按模式分叉的方法（拆分、类型目录、DML、DDL）读取这个字段。

### 4.2 版本号

- 人大金仓、openGauss 的模板版本改用 `server_version_num`：实测人大金仓 V9R2C16 为 120001（PG 12），openGauss 7.0.0 为 90204（PG 9.2.4）。
- 驱动在 `ServerInfo` 中另外带上这个值，由方言决定用哪个。
- 产品自己的版本号（`V009R002C016`、`7.0.0-RC1`）保留，用于显示。

### 4.3 新建连接按产品选

- 连接表单的"数据库类型"改为产品列表：MySQL、MariaDB、TiDB、OceanBase、PostgreSQL、人大金仓、openGauss、Oracle、达梦。
- 每项对应驱动、默认端口和可选的提示：
  - TiDB 4000；
  - OceanBase 2881，提示用户名写成 `用户@租户`；
  - 人大金仓 54321；
  - openGauss 5432。
- 连接配置增加可选字段 `product`，记录所选产品。
- 连上后以自动识别为准。识别出的变体与 `product` 不一致时，在连接信息中提示一句，方言按识别结果。
- 没有 `product` 的已有配置按驱动显示，行为不变。

## 5. 驱动

### 5.1 openGauss 的 SHA256 认证

- 把 `tokio-postgres` 与 `postgres-protocol` 的当前版本（与 `Cargo.lock` 一致）复制到 `vendor/`，在工作区 `Cargo.toml` 中用 `[patch.crates-io]` 替换。
- 补丁只改认证部分，补两种认证请求：
  - `AUTH_REQ_SHA256`（10）；
  - `AUTH_REQ_MD5_SHA256`（11）。
- 算法按 openGauss 的实现：用服务器给的盐和迭代次数做 PBKDF2，得到 ClientKey、StoredKey，再用令牌做 HMAC，与 ClientKey 异或后以十六进制发回。实现时以 openGauss 源码（`libpq` 的 `fe-auth.cpp` / `sha2.cpp`）为准。
- `vendor/PATCHES.md` 记录：对应的上游版本、改动的文件和函数、重新套用补丁的步骤。每个里程碑开头的依赖检查若发现上游新版本，按这份说明重新套用。
- 测试：
  - 单元测试用固定输入核对证明；
  - 实库测试用纯 SHA256 账号连接并执行一致性测试。

### 5.2 人大金仓

- 认证是标准的 SCRAM-SHA-256，现有驱动支持。
- Oracle 模式类型：连接后查一次 `pg_type`，把 `number`、`varchar2`、`date`、`clob`、`blob` 等类型的 OID 映射到已有的逻辑类型，缓存在连接上。
  - 映射规则：`date` 视为日期时间；`number` 视为定点数。
  - 没有映射的 OID 仍按文本读出。
- 一致性测试的类型用例按兼容模式分组（Oracle 模式不测 `'NaN'::numeric`）。

### 5.3 TiDB、OceanBase

- MySQL 驱动不变。
- OceanBase 的 `用户@租户` 原样传给服务器，直连 2881 和经 OBProxy 的 2883 都适用。

### 5.4 TLS

- 驱动的 TLS 支持不变。
- 接口层实库测试的 TLS 要求改为可按目标关闭。

## 6. 元数据与对象树

### 6.1 模板组织

- 新增覆盖文件 `sql/tidb.sql`、`sql/oceanbase.sql`、`sql/kingbase.sql`、`sql/opengauss.sql`，规则不变：同名 key 替换基础模板，正文为空表示没有这种查询。
- 模板上下文增加布尔值 `oracle_mode`。
- Oracle 模式下额外支持 `synonyms` key，`routines` 包含包（package）。对象树中的位置与 Oracle 家族一致，只读：列表和查看 DDL。

### 6.2 各库的差异

人大金仓：
- 版本改正后，PG 基础模板大部分适用。
- 覆盖的部分：
  - 系统 schema 过滤（`sys`、`sys_catalog`、`sysaudit`、`xlog_record_read` 等，以实库为准）；
  - 自增识别（序列加 `nextval` 默认值）；
  - Oracle 模式的同义词和包。
- 物化视图照常列出。授权不允许建物化视图时，测试跳过对应用例，产品不做处理。

openGauss：
- 分区信息改从 `pg_partition` 读，仍只读展示。
- 自增识别为 `serial` 或默认值为 `nextval(...)`。
- 系统 schema 过滤 `dbe_*`、`pkg_*`、`snapshot`、`blockchain`、`db4ai`、`sqladvisor` 等，以实库为准。
- A 模式的同义词和包。
- 其余报错的查询逐个改写。

TiDB：
- 对象树增加序列；表的存储信息增加聚簇索引（`CLUSTERED`）与 `AUTO_RANDOM`。
- 修正读回索引时"方法"的差异。

OceanBase：
- 修正读回索引的差异。
- 缺列或内容不同的 `information_schema` 视图，逐个改写。

### 6.3 验收

元数据实库测试（`dblens-dialect/tests/meta_live.rs`）在所有目标上通过。确实不支持的对象按目标跳过，并写明原因（例如 TiDB 没有触发器）。

## 7. 查询相关

### 7.1 语句拆分

- `Oracle` 模式的人大金仓和 openGauss 使用 Oracle 家族的块识别规则（`CREATE PROCEDURE/FUNCTION/PACKAGE/TRIGGER ... END;`，以及单独一行的 `/`），同时保留 PG 的 `$$` 引号识别。
- 后端 `split` 与前端拆分同步修改，共用一组用例。
- 编辑器的关键字与内置函数补全按模式切换：`Oracle` 模式加入 Oracle 的关键字与函数。

### 7.2 出错位置

- TiDB：新增解析规则，格式为 `line N column M near "..."`。
- OceanBase：调整 `near '...' at line N` 的解析，使其匹配 OceanBase 的报错。
- 人大金仓、openGauss：走 PG 的位置字段，由测试确认。

### 7.3 执行计划

- TiDB：表格结果。
- OceanBase：单列文本（`Query Plan`）。
- 人大金仓、openGauss：与 PG 相同。

### 7.4 运维查询

每个变体在 `sql/ops/` 下有自己的模板：
- TiDB：`information_schema.processlist`、`data_lock_waits`、`statements_summary`、`cluster_slow_query`；
- OceanBase：`GV$OB_PROCESSLIST`、`GV$OB_SQL_AUDIT`、`GV$OB_LOCKS` 等；
- openGauss：按实际存在的列改写 `pg_stat_activity` 相关查询，锁和慢 SQL 用 openGauss 的视图；
- 人大金仓：沿用 PG 模板，个别改用 `sys_` 前缀的视图。

没有数据来源的项不出现在该变体的运维查询列表中。

### 7.5 执行 SQL 文件

TiDB 的 "Unsupported type" 先定位到具体语句：用例语法问题就改用例，生成逻辑问题就修生成逻辑。

## 8. 数据编辑、导入导出与表设计器

### 8.1 自增列与默认值

- 自增识别修好后，沿用现有规则：用户没填值的自增列和有默认值的列，不出现在 INSERT 中。
- 兜底：有默认值且用户没有改动的列，即使没识别为自增，也不写入。

### 8.2 Oracle 模式下空串等于 NULL

- 数据网格不改变这一行为。在该连接上，编辑单元格的提示中说明"此库空串按 NULL 存储"。
- 提交后读回为 NULL 不算失败。
- UPDATE、DELETE 的行定位条件中，原值为空串时，`Oracle` 模式按 `IS NULL` 处理。

### 8.3 类型与字面量

- `Oracle` 模式的 `DATE` 按日期时间处理，`NUMBER` 按定点数处理。
- 生成字面量时，日期时间用 `TIMESTAMP '...'`、`DATE '...'`，不用 `::` 转换。

### 8.4 导入导出

- 流程不变。现有分批上限低于 TiDB 的事务大小上限和 OceanBase 的包大小上限。
- 导出 INSERT 按当前方言和模式生成，不做跨库转换。

### 8.5 表设计器

`design_caps` 按变体和模式给出：

| 库 | 类型目录 | 自增 | 调整列顺序 | 其他 |
|---|---|---|---|---|
| TiDB | MySQL | `AUTO_INCREMENT`，另有 `AUTO_RANDOM` | `MODIFY ... AFTER` | 删除或更换聚簇主键时走重建；检查约束在 `tidb_enable_check_constraint` 关闭时提示"只解析不执行" |
| OceanBase | MySQL | `AUTO_INCREMENT` | 按实测，不支持则重建 | 主键变更的限制按实测决定直接改或重建 |
| openGauss PG 模式 | PG | 新列 `serial`/`bigserial`；已有列改为自增时建序列并设默认值 | 重建 | DDL 在事务中执行 |
| openGauss A 模式 | PG 加 `NUMBER`、`VARCHAR2`、`CLOB`、`BLOB`、`DATE` 等 | 同上 | 重建 | 同上 |
| 人大金仓 PG 模式 | PG | `IDENTITY` | 重建 | 同 PG |
| 人大金仓 Oracle 模式 | PG 加 Oracle 风格类型 | `IDENTITY`；实测不可用时改为序列加默认值 | 重建 | 同 PG |

- DDL 生成复用 MySQL 家族、PG 家族的代码，只在上表列出的地方分叉。
- M3 的 `design_flow` 实库测试在所有目标上通过。

## 9. 测试环境与测试

### 9.1 测试库（虚拟机 192.168.139.106）

| 目标 | 端口 | 部署 | 环境变量 |
|---|---|---|---|
| TiDB v8.5.8 | 4000 | 官方二进制，单个 tidb-server（unistore），systemd `tidb` | `DBLENS_TEST_TIDB_URL` |
| OceanBase CE 4.4.2.1 | 2881 | 容器 `oceanbase`（quay.io，mini 模式），租户 `test` | `DBLENS_TEST_OCEANBASE_URL` |
| openGauss 7.0.0-RC1，PG 模式库 | 5433 | 容器 `opengauss`，库 `dblens_test` | `DBLENS_TEST_OPENGAUSS_URL` |
| openGauss，A 模式库 | 5433 | 同上，库 `dblens_test_a` | `DBLENS_TEST_OPENGAUSS_A_URL` |
| openGauss，纯 SHA256 账号 | 5433 | 同上，账号 `admin_sha` | `DBLENS_TEST_OPENGAUSS_SHA_URL` |
| 人大金仓 V9R2C16，Oracle 模式 | 54321 | DEB 包，systemd `kingbased`，开发版授权（365 天） | `DBLENS_TEST_KINGBASE_URL` |
| 人大金仓，PG 模式 | 54322 | 第二个实例，systemd `kingbased-pg` | `DBLENS_TEST_KINGBASE_PG_URL` |

每个目标有 `admin` 用户和 `dblens_test` 库。密码在虚拟机上随机生成，只保存在虚拟机的 root 文件中，不进仓库；运行测试时以环境变量临时传入。

### 9.2 测试组织

- MySQL 家族、PG 家族的实库测试改为遍历目标表。每个目标带一份"不支持项"说明（例如 TiDB 没有触发器、人大金仓开发版不能建物化视图）。
- 删除写死家族的断言，改为按目标核对识别出的变体和模式。
- 测试建表语句按目标选择自增写法：
  - openGauss 用 `serial`；
  - Oracle 模式的人大金仓用序列加默认值；
  - TiDB、OceanBase 用 `AUTO_INCREMENT`。
- 范围：驱动一致性、元数据、服务层全部流程、接口层。原有 5 个库与新增 7 个目标全部通过。

### 9.3 其他验证

- 单元测试：
  - SHA256 证明；
  - 兼容模式探测；
  - 版本号选择；
  - 拆分（Oracle 风格块与 `$$` 混用）；
  - 出错位置解析；
  - 各变体、模式的 DDL 快照。
- 前端：产品列表与默认端口、模式相关提示；tsc、Vitest、打包。
- 浏览器实测：
  - 每个产品新建连接一次；
  - 对象树、查询、编辑、设计器各操作一遍。
- 桌面程序的实测与之前延后的项一并进行。

## 10. 文档

- `docs/notes/domestic-batch1.md`：
  - 结构；
  - 各库的差异清单；
  - 与本设计不一致的地方；
  - 测试库的部署方式（不含密码）；
  - SHA256 补丁的升级说明，指向 `vendor/PATCHES.md`。
- 总体设计文档：更新进度和第二、三批的状态。
- `docs/notes/m0-4b-api.md`：连接配置的 `product` 字段。
