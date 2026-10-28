# 国产库适配第一批

依据：设计文档 `docs/superpowers/specs/2026-10-27-domestic-batch1-design.md`。

第一批是 TiDB、OceanBase 社区版（MySQL 模式）、openGauss（A、PG 兼容模式）、人大金仓 KingbaseES V9R2（Oracle、PG 模式），功能与 MySQL、PostgreSQL 相同。

## 结构

```text
crates/dblens-dialect/
  src/variant.rs      Compat（Native / Oracle）
  src/lib.rs          Dialect 带兼容模式；refine 读探测结果（模式、server_version_num）；split / splitter 按模式
  src/split.rs        StreamSplitter::with_oracle_blocks：PG 家族的 Oracle 兼容模式识别 PL/SQL 块和独占一行的 /
  sql/tidb.sql、oceanbase.sql、kingbase.sql、opengauss.sql   元数据模板的覆盖文件
  sql/ops/tidb.sql、oceanbase.sql、opengauss.sql             运维查询的覆盖文件
  src/design/catalog.rs、sql.rs、postgres.rs                  设计器的能力表、TiDB 主键、openGauss 的 serial
  src/import.rs、changes.rs                                   openGauss 的按主键更新；Oracle 模式下空串按 IS NULL 定位
crates/dblens-driver-postgres/src/types.rs   兼容库类型（域、clob、int1、int16 等）按标准类型读写
crates/dblens-driver-mysql/src/error.rs      OceanBase、TiDB 报错的出错位置
crates/dblens-service/src/registry.rs        测试连接返回兼容模式；profile.rs、store.rs 记录产品（迁移版本 4）
vendor/tokio-postgres、vendor/postgres-protocol   openGauss 的 SHA256 认证补丁（vendor/PATCHES.md）
web/src/features/connections/products.ts      按产品新建连接
web/src/features/sql/dialects.ts、editor/dialect.ts   Oracle 兼容模式按 PL/SQL 的语法、补全、拆分、高亮
```

## 各库的差异

TiDB（v8.5.8，`VERSION()` 报 8.0.11）：
- 没有存储过程、触发器、全文索引；有序列（`information_schema.SEQUENCES`）。
- CHECK 约束只在 `tidb_enable_check_constraint` 打开时保存，且不在 `TABLE_CONSTRAINTS` 里，改从 `TIDB_CHECK_CONSTRAINTS` 读。
- 对 `information_schema` 做相关子查询，匹配多行时结果全是 NULL；表的分区改用 LEFT JOIN 读取。
- `AUTO_RANDOM` 不在 `EXTRA` 里，由表的 `TIDB_ROW_ID_SHARDING_INFO` 判断；表存储信息带 `clustered`（`TIDB_PK_TYPE`）。
- 接受 `DESC` 但按升序建索引；聚簇主键不能删除或更换。
- 锁等待从 `DATA_LOCK_WAITS` 与 `CLUSTER_TIDB_TRX` 读；报错格式为 `line N column M near "…"`。
- 自增值按批分配，重建表后新值大于原最大值，但不一定紧接着。

OceanBase 社区版（4.4.2.1，MySQL 模式，`VERSION()` 报 5.7.25）：
- 执行 CHECK 约束，按 MySQL 8.0.16+ 的方式读取。
- `lower_case_table_names = 1`，表名存成小写。
- 分区表的 `CREATE_OPTIONS` 为 NULL，分区改用 LEFT JOIN 读取。
- 全文索引在 `STATISTICS` 中只有隐藏列（`__doc_id_*`、`__word_segment_*`），去掉后不列出。
- 接受 `DESC` 但按升序建索引。
- `TRIGGERS.ACTION_STATEMENT` 是 `CREATE` 之后的整条语句，不只是触发器体。
- 没有 `information_schema.KEYWORDS`、sys schema 和 `performance_schema` 的语句摘要：保留字用内置列表，锁从 `GV$OB_LOCKS`、慢 SQL 从 `GV$OB_PLAN_CACHE_PLAN_STAT` 读。
- 报错信息后另起一行附上节点和追踪号。

openGauss（7.0.0-RC1，基于 PG 9.2.4）：
- 认证：远程登录常用 SHA256（请求码 10，与 PG 的 SASL 相同），见 `vendor/PATCHES.md`。
- 没有 IDENTITY 列：设计器的自增列用 `serial` / `bigserial` / `smallserial`，已有列改成自增时建序列并设默认值。
- 没有 `LATERAL`、`WITH ORDINALITY`、`relispartition`、`pg_sequence`、`pg_get_partkeydef`、`attidentity`、`attgenerated`：
  - 分区在 `pg_partition` 中，分区表在 `pg_class` 里是 `relkind = 'r'`、`parttype = 'p'`；
  - 生成列由 `pg_attrdef.adgencol = 's'` 标记；
  - 序列参数用 `pg_sequence_parameters`，读不到缓存大小。
- 聚合函数的 `prokind` 也是 `f`，仍要用 `proisagg` 排除；包里的函数 `propackageid` 不为 0。
- 同义词在 `pg_synonym`，包在 `gs_package`（只有 A 模式能建）。
- 没有 `ON CONFLICT`，按主键更新用 `ON DUPLICATE KEY UPDATE … = EXCLUDED.…`。
- `pg_stat_activity` 没有 `backend_type`、`wait_event_type`；没有 `pg_blocking_pids`；语句统计在 `dbe_perf.statement`（需要 MONADMIN 权限）。
- LIST 分区键不能是 text；没有 EXCLUDE 约束；numeric 的小数位不能为负。
- 新建库默认是 A（Oracle 兼容）模式。

人大金仓 KingbaseES（V9R2C16，基于 PG 12）：
- 版本字符串 `V009R002C016` 会被解析成 9.2，改用 `server_version_num`（120001）。
- 两种模式的系统目录都是 PG 的，PG 模板大部分可直接用。
- Oracle 模式下空串就是 NULL：PG 模板中与 `''` 的比较全部改写（`attidentity IN ('a', 'd')`、`attgenerated = 's'`）。
- Oracle 模式的 `DATE` 是 timestamp 上的域，`NUMBER` 读出来就是 numeric；`'NaN'::numeric` 不被接受。
- 同义词在 `sys_synonym`，包在 `_pkg`（`pkgspecsrc` / `pkgbodysrc`）。
- 开发版授权：最多 10 个连接，不能建物化视图。
- 没有加载 `sys_stat_statements`，慢 SQL 查询报错（与 PG 测试库没有 `pg_stat_statements` 相同）。

## 与设计文档不一致的地方

- 所基于的 PG 版本（4.2 节）由探测查询一并读出（`current_setting('server_version_num')`），`ServerInfo` 没有加字段。
- openGauss 的 MD5_SHA256（请求码 11）不实现（5.1 节）：服务端计算 MD5 摘要时包含了存储串末尾的迭代次数后缀，客户端收不到这个后缀，无法通过认证（openGauss 自己的 libpq 也是按不含后缀计算）。驱动遇到时报出明确的错误，提示把 pg_hba 改为 sha256。
- 兼容库类型（5.2 节）不另查 `pg_type`：tokio-postgres 解析未知类型时已经查过，域类型取基础类型，其余按类型名（clob、nvarchar2、blob、raw、number、int1、tinyint、int16、smalldatetime 等）对应到标准类型；参数也按对应的标准类型编码。
- 同义词与包（6.1 节）在两种模式下都列出：两个库的目录在 PG 模式下也有同义词，包在 PG 模式下为空。
- openGauss 读不到序列的缓存大小。
- OceanBase 的全文索引不列出（只有隐藏列）。
- TiDB 删除或更换主键一律重建（设计文档写"聚簇主键"）：默认建的主键都是聚簇的，非聚簇主键重建也正确，只是更慢。选项页显示是否聚簇。
- TiDB 的检查约束在设计器中按"只解析不执行"提示，不看 `tidb_enable_check_constraint` 的当前值。
- 插入时有默认值的列留空就不写（8.1 节的兜底），M2a 时已经是这个规则，没有改动。
- 编辑器（7.1 节）：Oracle 兼容模式下按 PL/SQL 的语法（兼容档，只标黄）、关键字补全和拆分处理。在 Oracle 模式里用 `$$` 写 PG 风格的函数时，编辑器的"执行当前语句"可能选错范围；后端执行（含执行 SQL 文件）两种写法都正确拆分。
- 运维查询中 OceanBase 的锁等待按文档写成，测试库上只验证了能执行，没有构造锁等待核对内容。
- 所选产品与识别结果不一致的提示：选的是标准库（MySQL、PostgreSQL、Oracle、达梦）时不提示，较早保存的连接都是按驱动选的。

## 测试库

在测试虚拟机 192.168.139.106 上（密码在虚拟机 root 的 `/root/dblens-test/creds.env`，不进仓库；运行测试时以环境变量传入）：

| 目标 | 端口 | 部署 |
|---|---|---|
| TiDB v8.5.8 | 4000 | tiup 镜像站的官方二进制，单个 tidb-server（`-store unistore -path /data/tidb`），systemd `tidb`，`WorkingDirectory=/data/tidb`（慢日志用相对路径） |
| OceanBase CE 4.4.2.1 | 2881 | `quay.io/oceanbase/oceanbase-ce:4.4.2-lts`，`MODE=mini`，必须加 `OB_SERVER_IP=127.0.0.1` 和 `--ulimit nofile=65535:65535`，租户 `test` |
| openGauss 7.0.0-RC1 | 5433 | Docker Hub 的 `opengauss/opengauss:7.0.0-RC1`（经华为云 SWR 同步副本拉取；quay.io 的 7.0.0-RC3 镜像缺动态库）；`password_encryption_type = 1`，远程认证 sha256；库 `dblens_test`（PG）、`dblens_test_a`（A）、`dblens_test_sha`（PG，属于 `admin_sha`：一致性测试用固定的表名，与 `dblens_test` 上的测试同时跑会互相删表）；账号 `admin`（MONADMIN）、`admin_sha`（只存 SHA256）、`admin_m5s`（pg_hba 为 md5，用于确认不支持的报错） |
| 人大金仓 V9R2C16 | 54321 / 54322 | 官方 DEB 包，开发版授权（365 天）；Oracle 模式 systemd `kingbased`，PG 模式第二个实例 `kingbased-pg`（`initdb -m pg`） |

实库测试的环境变量见设计文档 9.1 节。人大金仓的测试要串行运行（`--test-threads=1`，授权限制 10 个连接）；TiDB 的元数据测试会把 `tidb_enable_check_constraint` 设为 ON。

## 验证

- 后端：fmt、clippy 通过；离线测试全部通过。
  - dialect：兼容模式探测与版本、Oracle 模式拆分、各变体元数据模板的快照、旧版本分支能渲染、设计器能力表、openGauss 的按主键更新、Oracle 模式的空串定位。
  - 驱动：openGauss SHA256 证明（固定输入）、请求报文解析、TiDB 和 OceanBase 的出错位置。
- 实库（原有 5 个库与新增 7 个目标）：驱动一致性、元数据、服务层全部流程（会话、工作台、取消、对象操作、运维、导出、编辑、导入、执行 SQL 文件、表设计器）、接口层。
  - 达梦和其他测试同在一个进程里运行时偶发"网络通讯失败"，单独运行通过（与以前的记录相同）。
- 前端：tsc、Vitest、打包通过。
- 浏览器实测（中文界面）：
  - 新建连接按产品切换时端口和提示正确（OceanBase 2881 与"用户@租户"、人大金仓 54321、openGauss、TiDB 4000）；测试连接接口返回的兼容模式和版本（人大金仓 oracle / pg、120100；openGauss A、90204）。
  - 人大金仓 Oracle 模式的查询标签：编辑器切换为 PL/SQL，执行全部时 Oracle 风格的过程块按 `/` 拆分，建表、建过程、调用、查询都成功，空串存成 NULL；数据标签显示"此库空串按 NULL 存储"。
  - 设计器能力：openGauss A 模式有 VARCHAR2，PG 模式没有；TiDB 不建降序索引、检查约束按只解析不执行；TiDB 表的存储信息带聚簇主键。
  - 实测中发现并修正：记了产品、但没连接过的连接，对象树和状态栏仍按驱动显示（如"PostgreSQL 系"），改为按选的产品显示（`profileKind`）。
- 未验证：桌面程序中的连接对话框和编辑器没有单独实测（并入之前延后的桌面实测）；设计器在新库上的界面操作只用接口核对，DDL 的生成和执行由实库测试覆盖。
