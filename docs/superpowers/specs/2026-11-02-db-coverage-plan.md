# 数据库覆盖计划

2026-11-02 与用户确认方向：专注国产库，不求大而全，把用户常见的国内外关系型数据库覆盖好。规划按“产品、模式、主流版本”列，不只按协议。SQLite 要支持；DB2、虚谷等不做。

## 现状

| 产品 | 模式 | 实测版本 |
|---|---|---|
| 达梦 | `COMPATIBLE_MODE` 0、2、3、4、7；大小写敏感和不敏感 | DM8 测试库 8.1.5.60；用户真实库 DM8 0x7000c（说明 `docs/notes/dameng-compat.md`） |
| 人大金仓 | Oracle、PG | V9R2C16 |
| openGauss | A、PG | 5.0.0、6.0.6、7.0.0 LTS（说明 `docs/notes/opengauss-lts.md`） |
| GaussDB | A、PG | 混合云开发者版，内核 507.0（说明 `docs/notes/gaussdb.md`） |
| OceanBase | MySQL | 社区版 4.4.2.1 |
| OceanBase | Oracle | 企业版试用 4.4.2.2 |
| GoldenDB | Oracle、MySQL 租户 | V6.1.03.12SP5 |
| TiDB | — | v8.5.8 |
| GBase 8s | — | V8.8_3.6.5 开发版 |
| MySQL | — | 9.7.4 |
| MariaDB | — | 12.3.3 |
| PostgreSQL | — | 18.6 |
| Oracle | — | 26ai Free 23.26.3；11g XE 11.2.0.2（说明 `docs/notes/oracle-11g.md`） |
| SQL Server | — | 2017、2022、2025 |

除 SQL Server、达梦外，每个库只测过一个版本，而且多是最新版。现场常见的是老版本。

## 差异在哪里

- 部署形态（云版、社区版、企业版）：引擎和协议相同，差在连接层和权限（代理、没有超级权限）。选择列表可以分开列，代码不分。
- 兼容模式：数据字典、语法、空串语义都不同，必须分开做和测。
- 集中式和分布式：查询浏览一样，建表多了分布键，读回的 DDL、表设计器、迁移要处理。
- 大版本：数据字典视图和语法有增减（Oracle 12c 的自增列和 `FETCH FIRST`，MySQL 8.0 的 CHECK 约束和窗口函数），要逐版实测。

## 各项

### 达梦 `COMPATIBLE_MODE`

代码里没有处理，测试库都是 0。取值：0 不兼容，1 SQL92，2 Oracle，3 SQL Server，4 MySQL，5 DM6，6 Teradata，7 PG。它是 `dm.ini` 的静态参数，重启生效，影响语法和语义（比如 Oracle 模式下空串当 NULL）。我们读数据字典用的 `ALL_*` 视图不受影响，风险在生成的 SQL 和数据编辑。

另一个影响更大的是建库时的 `CASE_SENSITIVE`（大小写敏感，默认是），现场常有建成不敏感的库，标识符加不加引号行为不同。

做法：用 5237 实例依次切到 2、3、4、7 跑服务层全流程；另建一个 `CASE_SENSITIVE=N` 的实例跑一遍。2026-11-03 完成，见 `docs/notes/dameng-compat.md`。

### 人大金仓

| 版本 | 说明 |
|---|---|
| V8R6 | 现场装得最多，必须测 |
| V9R1C10 | 一个版本有 Oracle、MySQL、SQL Server、PG 四种兼容模式，有 Docker 包（x64、海光、飞腾、兆芯） |
| V9R2C16 Oracle 兼容版 | 已测 |
| V9R3C18 MySQL 兼容版、V9R4C19 SQL Server 兼容版 | 用 V9R1C10 的对应模式先覆盖，有差异再单独装 |

代码里金仓的兼容模式只有 Oracle 和原生（PG），要加 MySQL、SQL Server 两种：协议仍是 PG，标识符引用、类型名、分页、过程块语法跟着模式变。

2026-11-04 实测（见 `docs/notes/kingbase-versions.md`）：两种模式下 PG 写法基本都能用，不另做方言，生成的 SQL 避开它们改变语义的写法即可。V8R6、V9R1C10 的 oracle、pg、mysql 模式全量通过；sqlserver 模式的自增列和 time、datetime2 类型未解决。

### openGauss

现有 7.0.0-RC1 换成 7.0.0 LTS，再加 6.0.6 LTS 和 5.0 LTS。每个版本测 A、PG 两种模式和 SHA256 认证。2026-11-04 完成，见 `docs/notes/opengauss-lts.md`。

### GaussDB

用户给的下载页（华为云 GaussDB 下载）是混合云开发者版，集中式，就是现在 `gaussdb-dev` 容器装的那一类。下一步正式适配：服务层全流程在 A、PG 两种模式下跑通，修掉已知问题（读不到角色成员关系，权限页不完整），确认下载页最新版本后换装。分布式版和 DWS 没有免费安装包，先不做。2026-11-03 完成，见 `docs/notes/gaussdb.md`。

### Oracle

2026-11-03 与用户确认：和 Navicat 一样只有一个 Oracle 入口，连上后按版本号走不同的查询，不按版本分产品。测试只覆盖版本分支的节点：11.2（老基线）、19c（现场最多；18c、19c 都是 12.2 的后续版本，字典与 12.2 基本相同）、23ai（新类型）。12.1、18c、21c 不单独测。11.2 用用户提供的 11.2.0.4 企业版介质（XE 版没有分区、位图索引）。

| 版本 | 测试环境 |
|---|---|
| 11.2 | `gvenzl/oracle-xe:11`（XE） |
| 12c、19c | 要 Oracle 账号下载安装包，按官方 Docker 脚本构建 |
| 21c | `gvenzl/oracle-xe:21` |

已知要改：11g 没有 `OFFSET … FETCH`，分页要改 ROWNUM（`dblens-dialect/src/literal.rs` 写明未实现）；12c 以前没有自增列和 `ALL_IDENTITY_COLUMNS`；12.2 以前标识符最长 30 字节。客户端兼容也要验证：新版 Instant Client 对老库的支持范围要核实，不行就让 11g 走 JDBC 桥接。

### MySQL、PostgreSQL 老版本

MySQL 5.7、8.0；PostgreSQL 12、14。元数据读取写了 8.0.13、8.0.16 以前的分支，但没在老版本上跑过。

MySQL 5.7.44、8.0.44 于 2026-11-04 验证，见 `docs/notes/mysql-versions.md`；5.7 的 TLS 握手（服务器只有 DHE、RSA 密钥交换）未解决。

### SQLite

新驱动，单独一个里程碑：本地文件连接，浏览、查询、数据编辑、导入导出、表设计器（SQLite 的 `ALTER TABLE` 能力有限，多数改动要重建表）、结构比对，可作迁移的源和目标。

## 测试机安排

- 106 内存 23G，磁盘只剩 18G：Docker 占 52G（数据卷 45G）。虚拟机文件在 D 盘，D 盘只剩 32G。
- 已验证的库平时不启动，用时再起；PG（5432）和达梦（5236）常开，审计版测试要用。
- 新库一个一个装，验完记下结果和安装步骤后停掉；不常用的老版本验完可以删掉，留安装记录。
- 磁盘不够时先清理不再用的镜像（Keycloak、rockylinux:8 等），还不够要扩虚拟机磁盘，或把虚拟机挪到空间更大的 E 盘，由用户决定。

## 顺序

1. Oracle 11g、19c（信创的主场景是从 Oracle 迁出）。11g 已完成（2026-11-03），19c 等安装包
2. 金仓 V8R6，V9R1C10 的四种模式。已完成（2026-11-04），sqlserver 模式的自增列待定
3. MySQL 5.7、8.0。已完成（2026-11-04），5.7 的 TLS 待定
4. openGauss 5.0、6.0.6、7.0.0 LTS。已完成（2026-11-04）
5. GaussDB 正式适配。已完成（2026-11-03）
6. 达梦兼容模式和大小写不敏感。已完成（2026-11-03）
7. PostgreSQL 12、14
8. SQLite

要用户提供的：金仓、Oracle 19c 等需要登录才能下载的安装包。
