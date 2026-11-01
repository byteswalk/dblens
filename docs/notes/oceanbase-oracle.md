# OceanBase 的 Oracle 模式

国产库第三批的最后一项。没有写设计文档：用现有 Oracle 方言实跑后，只有下面几处不同。

## 怎么连

标准 MySQL 客户端连不上 Oracle 模式租户，服务器直接拒绝：`Oracle tenant for current client driver is not supported`。所以走 JDBC 桥接，用官方驱动 `oceanbase-client`（LGPL-2.1，随包发布，见 `scripts/clients.manifest.json`）。

- 连接配置：`driver=oracle`、`client=jdbc`、`jdbc_kind=oceanbase`，产品 `oceanbase_oracle`。用户名写成 `用户@租户`，`database` 是模式名，可留空。
- 驱动报告的产品名是 Oracle，版本是 OceanBase 自己的。`JdbcConnection` 改成报告 OceanBase，由此识别出 `Variant::OceanBaseOracle`，家族仍是 `Family::Oracle`。

## 和 Oracle 不同的地方

| 项目 | 处理 |
|---|---|
| 版本号 4.4 不能和 Oracle 比 | `Dialect::oracle_release()` 返回 None，判断标识符长度、自增时用它；元数据模板仍走 11.2 基线 |
| 执行计划 | 没有 `EXPLAIN PLAN FOR`，用 `EXPLAIN` |
| 系统账号 | 没有 `ORACLE_MAINTAINED`，按名单判断 |
| 自增列 | `IDENTITY_COLUMN` 是空的，默认值 `SEQUENCE.NEXTVAL` 表示自增 |
| 运维查询 | 没有 `V$SESSION` 等，用 `V$OB_PROCESSLIST`、`V$OB_LOCKS`、`V$OB_UNITS`，杀会话用 `KILL` |
| 权限 | `DBA_TAB_PRIVS` 没有 `TYPE`；`ALL_COL_PRIVS` 的属主列叫 `OWNER` |
| 触发器 | `DESCRIPTION` 是空的，`TRIGGER_BODY` 是完整定义，少最后的分号 |
| 降序索引 | 不支持，`descending_index` 能力关闭 |
| SQL 错误位置 | 只报 `near '…'`，没有偏移，编辑器不标红 |

## 自增列不能续接

`ALTER TABLE … MODIFY … (START WITH …)` 语法不支持，系统序列 `ISEQ$$_…` 也不让改（`cannot alter a system-generated sequence`）。所以：

- **表设计器**：带自增列的表拒绝重建，留着会让新值撞主键。
- **迁移到这里**：自增列迁成普通列，并给有损提示。
- **删用户**：必须带 `CASCADE`，不勾选时报服务器原文，不替用户自动级联。

## 已验证

实库：OceanBase 企业版试用 4.4.2.2，Oracle 模式租户。

- 服务层 10 个流程：完整流程、工作台、对象、运维、导出、数据编辑、导入、SQL 文件、表设计器、模型。
- 账号权限（查看和修改）、规范检查的存储信息查询。
- 迁移：MySQL → 这里，这里 → MySQL，这里 → Oracle。
- 真实 Oracle 的 10 个流程回归通过。
- 真实鼠标（浏览器模式）：连接表单、对象树、执行查询。

## 待验证

- 桌面版里新建这种连接。
- 打包：发版时 `DBLENS_CLIENTS_DIR` 里要放 `oceanbase-client-2.4.14.jar`。
- 分布式部署、OBProxy 连接（端口一般 2883）：只测过单机直连。

## 测试环境

VM .106 的容器 `ob-ee`（Rocky 9，端口 12881，`admin` 用户部署，集群名 `obee`）。试用许可证 180 天，到期后不能查询和建表，要重装。VM 重启后容器不会自动起，起来后还要 `obd cluster start obee`，并设 `fs.aio-max-nr=1048576`。
