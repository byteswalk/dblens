# 人大金仓 V8R6、V9R1C10

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第二项。原来只测过 V9R2C16（Oracle、PG 两种模式）。

## 测试库

VM .106 上的 Docker 容器，镜像是金仓官网的 Docker 包（只含数据库本体），换上了官网的开发版授权（365 天，限 10 个连接，测试按单线程跑）。

| 版本 | 模式 | 容器 | 端口 |
|---|---|---|---|
| V008R006C009B0014 | oracle | `kb86` | 54331 |
| V008R006C009B0014 | pg | `kb86pg` | 54332 |
| V009R001C010B0004 | oracle | `kb91oracle` | 54341 |
| V009R001C010B0004 | pg | `kb91pg` | 54342 |
| V009R001C010B0004 | mysql | `kb91mysql` | 54343 |
| V009R001C010B0004 | sqlserver | `kb91sqlserver` | 54344 |

- 镜像的入口脚本默认 `ENABLE_CI=yes`（大小写不敏感）；oracle、pg 模式建库时改成 `no`，和原来的测试库一致。mysql、sqlserver 模式强制大小写不敏感。
- 每个实例有超级用户 `admin` 和库 `dblens_test`，密码在 VM 的 `creds.env`（`KB86_*`、`KB86PG_*`、`KB91ORA_*`、`KB91PG_*`、`KB91MYS_*`、`KB91SQL_*`）。
- 本机连接串 `DBLENS_TEST_KB86_URL`、`KB86PG`、`KB91ORA`、`KB91PG`、`KB91MYS`、`KB91SQL`。跑测试时赋给 `DBLENS_TEST_KINGBASE_URL`（oracle 模式）、`DBLENS_TEST_KINGBASE_PG_URL`（pg 模式；mysql、sqlserver 模式也按这一组测试跑），驱动一致性测试另有 `DBLENS_TEST_KINGBASE_MYSQL_URL`、`DBLENS_TEST_KINGBASE_SQLSERVER_URL`。

## MySQL 模式

协议、系统目录和 PG 模式相同，双引号标识符、`LIMIT … OFFSET` 也能用，所以不另做方言，按原生（PG）规则处理，只是生成的 SQL 避开它改变语义的写法（见下表）。另外：

- 日期时间、字符、二进制是 `sys` 模式下的同名类型，OID 不同，驱动按类型名映射到标准类型。
- AUTO_INCREMENT 列在系统目录里的 `attidentity` 是 `'i'`，读表结构时算作自增。
- 把 numeric 显示成 `number`（如检查约束里的 `(0)::number`），迁移到其他 PG 系库时改回 `numeric`。
- `serial` 按 MySQL 的含义带 UNIQUE。

## SQL Server 模式

方言另有兼容模式 `sqlserver`（`Compat::SqlServer`），大体按 PG 规则，只有下面这些不同：

- 自增列写 `IDENTITY(1, 1)`，系统目录里 `attidentity` 是 `'q'`。已有列加、去自增改为重建表。往自增列写指定值要 `SET IDENTITY_INSERT … ON`（迁移、重建表都按这个写）；写入后标识值自动推进。
- 加列写 `ADD` 不带 `COLUMN`；其余 `ALTER COLUMN … TYPE`、`RENAME COLUMN`、`COMMENT ON` 等 PG 写法都能用。
- `timestamp` 是行版本。不带时区的时间戳建成 `datetime2`，默认当前时间写 `CURRENT_TIMESTAMP`（`LOCALTIMESTAMP`、`now()` 建表不报错，插入时报类型不符）。设计器的类型列表去掉 timestamp，加上 datetime2 等。
- 没有生成列：设计器不显示生成列，迁移时只建普通列并提示。
- `time`、`datetime2` 按 100 纳秒计（PG 按微秒），`datetime2` 从 2000-01-01 起算；名字和 MySQL 模式的同名类型相同，驱动第一次遇到时按发送函数名（`sqlserver_time_send`、`sqlserver_datetime2_send`）查出 OID 再换算。
- 另有 SQL Server 的保留字，标识符按这张表也加引号；补上 `OUTPUT`、`RAISE`。
- PL/pgSQL 不能写这个模式建的表，过程用 T-SQL 写（`CREATE [OR ALTER] PROCEDURE … AS BEGIN … END`，`EXEC` 调用）。SQL 文件和编辑器分句时，过程和 Oracle 兼容模式的 PL/SQL 块一样以独占一行的 `/` 结束。
- `bit`、`money`、`rowversion`、`sql_variant` 等类型原样显示。

## 两种模式共同的写法

| 写法 | MySQL 模式 | SQL Server 模式 | 改成 |
|---|---|---|---|
| `'a' \|\| 'b'` | 逻辑或，得到布尔值 | 没有这个运算符 | `OPERATOR(pg_catalog.\|\|)` |
| `::numeric` | `DECIMAL(10,0)`，截掉小数 | 同左 | `::pg_catalog.numeric` |
| `confkey[k.pos]`、`int2[]` | 可用 | 方括号是标识符引号 | 按序号取的子查询、`pg_catalog._int2` |
| `BEGIN` | 可用 | 语法错误 | `START TRANSACTION` |

## 改了什么（其他）

- 保留字补上 V8R6 的 `AUTO_INCREMENT`、`CONNECT_BY_ISCYCLE`、`CONNECT_BY_ISLEAF`、`NVL`、`NVL2`、`SYS_CONNECT_BY_PATH`、`UROWID`，MySQL 模式的 `BEGIN`、`UNIX_TIMESTAMP`。

## 测试

- V8R6、V9R1C10 的 oracle、pg 模式：工作区全量通过，包括和 PostgreSQL、达梦之间的双向迁移。
- V9R1C10 的 mysql、sqlserver 模式：按 pg 模式那一组测试全量通过（sqlserver 模式下 PG 写的数据类型表、数组、GIN 索引、生成列不建；驱动一致性测试另有一组），另测了从 PostgreSQL、达梦迁入。
- 回归：PostgreSQL 18、openGauss 7.0、MySQL 8.0、达梦全量通过。
