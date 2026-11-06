# Oracle 11g

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第一项。元数据模板原本就以 11.2 为基线写了版本判断，但从没在 11g 上跑过。

测试库有两个。企业版 11.2.0.4（用户提供的安装介质）：VM .106 的容器 `ora11204`（centos:7，必须带 `--init`，否则后台进程退出后成僵尸，实例起停报 ORA-00600），端口 1523，SID `ORCL`，软件和数据在 `/data/ora11204`，连接串 `DBLENS_TEST_ORACLE11204_URL`。建库用 `New_Database` 模板从零建、关掉 JVM 等组件：用通用模板克隆种子库时库内 Java 报 ORA-29516、建库中断，数据字典留在 US7ASCII，中文在 SQL 里比较不到。XE 版 11.2.0.2：容器 `oracle-xe11`（`ghcr.io/gvenzl/oracle-xe:11`，Oracle Database 11g Express Edition 11.2.0.2），端口 1522，服务名 `XE`，用户 `DBLENS_TEST`（DBA）。连接串 `DBLENS_TEST_ORACLE11_URL`（原生客户端）、`DBLENS_TEST_ORACLE11_JDBC_URL`（JDBC 桥接）在本机 `~/.dblens-test`。跑 11g 时把其中一个赋给 `DBLENS_TEST_ORACLE_URL`。

## 客户端

- 原生：Instant Client 23.26 能连 11.2，不用换老版本客户端。
- JDBC：随包的 ojdbc11 23.26.4 也能连 11.2。

## 改了什么

| 问题 | 处理 |
|---|---|
| 11g 没有 `OFFSET … FETCH` | `Dialect::paginate` 在 12.1 以前改用 ROWNUM（`literal::paginate_rownum`）。只取前几行时不加序号列，有偏移时多一列 `DBLENS_RN` |
| `ALL_CONSTRAINTS.INDEX_OWNER` 为空（`USER_CONSTRAINTS` 里有值），主键、唯一约束背后的索引被当成普通索引，同步时会去删它（ORA-02429） | 索引查询改用 `NOT EXISTS`，属主为空时按约束的属主算。OceanBase 的 Oracle 模式沿用这条查询 |
| 没有标识列 | 设计能力加 `auto_increment`，12.1 以前为否：设计器里自增勾选框不可用，服务端校验报错 |
| 迁移目标没有标识列时整个迁移被拦下 | 改成有损提示：自增列迁成普通列，数据照迁，提示迁完后自建序列和触发器 |
| 权限页的内置账号、角色没有标记（没有 `ORACLE_MAINTAINED`） | 账号用元数据 `[schemas]` 同一份系统账号名单；角色按“SYS 带管理选项持有”判断 |

## 测试

`meta_live` 的 Oracle 夹具用了 12c 以后的写法，11g 时替换 `dept`、`emp` 两段，跳过标识列、23ai 类型、和虚拟列同表达式的函数索引（ORA-54018）；XE 版另外跳过分区和位图索引。服务层的数据编辑、表设计、迁移测试在 11g 上用序列加触发器代替自增列。

结果：

- 11g XE 原生客户端、JDBC 桥接各跑一遍工作区全量测试，全部通过。
- 11.2.0.4 企业版原生客户端全量通过，含分区、位图索引；和 PG、达梦之间的迁移六条路线通过。
- 迁移：PG、达梦和 11g 之间双向都通过。目标是 11g 时自增列迁成普通列；源是 11g 时源表本来就没有自增列。
- 回归：PG、达梦、MySQL、Oracle 26ai、OceanBase Oracle 模式全量。26ai 的 `privilege_live` 原先失败，是测试把 schema 写死成 `DBLENS_TEST`，而 26ai 的测试连接 10 月 31 日起改用 `DBLENS_JDBC` 账号：表建在 `DBLENS_JDBC` 下，报告查的是 `DBLENS_TEST` 里不存在的同名表。测试改为按连接账号取 schema 后，26ai、11g、达梦都通过。
- 浏览器模式真实鼠标：11g 连接的对象树、新建表（自增勾选框不可用，主键正常，建表成功）、查询 `v$version`。
- 顺手修了设计器“选项”里 schema 下拉框没有宽度、没选值时缩成一条的问题。

## 限制

- Oracle 走 JDBC 时取不到出错位置（原生客户端能取到），和版本无关。
- 用序列加触发器实现的“自增”识别不出来，元数据里这一列不是自增列。
- 版本只测 11.2、19c、23ai 三个节点（见覆盖计划），19c 见 `docs/notes/oracle-19c.md`。
