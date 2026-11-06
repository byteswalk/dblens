# Oracle 19c

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）第一项的后半部分。19c 是 12.2 的后续版本，现场最多。

## 测试库

Oracle Database 19c 企业版 19.3.0.0.0，用户提供的 rpm 安装包（`oracle-database-ee-19c-1.0-1.x86_64.rpm`，加上 preinstall、compat-libstdc++-33 两个依赖包）。EE 和 preinstall 包用 Oracle 的 OL7 公钥校验签名通过。

- VM .106 的容器 `ora19c`：centos:7，带 `--init`（和 11.2.0.4 一样，没有它后台进程会成僵尸），端口 1524，软件和数据在 `/data/ora19c`（挂到 `/opt/oracle`）。
- CDB `ORCLCDB`，PDB `ORCLPDB1`，字符集 AL32UTF8，内存 2G。测试用户 `DBLENS_TEST`（DBA）建在 PDB 里。
- 连接串在本机 `~/.dblens-test`：`DBLENS_TEST_ORACLE19_URL`（原生客户端）、`DBLENS_TEST_ORACLE19_JDBC_URL`（JDBC 桥接），服务名 `ORCLPDB1`。跑 19c 时赋给 `DBLENS_TEST_ORACLE_URL`。

安装要点：

- compat-libcap1 的本地包是 CentOS 6 的签名，改用 CentOS 7 源里的同名包。
- 挂进来的 `/opt/oracle` 属主要先改成 `oracle:oinstall`，否则 EE 包的安装前脚本报错。
- 容器里 `su` 开不了会话，安装时设 `ORACLE_DOCKER_INSTALL=true`；建库不用 rpm 带的 configure 脚本，直接以 oracle 身份运行 `dbca -silent`（大约 20 分钟）。

## 改了什么

| 问题 | 处理 |
|---|---|
| 索引查询用了 `ALL_TAB_COLS.DATA_DEFAULT_VC` 读函数索引的表达式，这一列 23ai 才有，19c 报 ORA-00904，元数据、设计、迁移全部失败 | 版本判断从 12.2 改成 23：23 以前和 11g 一样从 `ALL_IND_EXPRESSIONS` 读 LONG 表达式 |
| 夹具里和虚拟列同表达式的函数索引（`UPPER(name)`）在 19c 报 ORA-54018，23ai 允许 | `meta_live` 在 23 以前跳过这个索引和 23ai 类型的表 |

## 测试

- 19c 原生客户端、JDBC 桥接各跑一遍 Oracle 相关测试（74 个），全部通过，含分区、位图索引、标识列、和 PG、达梦之间的迁移。
- 回归：26ai 的 Oracle 测试全部通过，方言快照更新。
- 第一次在 19c 上跑全量时，除了 Oracle 的测试都通过。

## 限制

- 只测了 19.3 基础版本，没打 RU 补丁。
- 12.1、18c、21c 没有单独测（见覆盖计划）。`DATA_DEFAULT_VC` 是否在 21c 已有没有核实，21c 按 23 以前处理，结果一样，只是走老查询。
