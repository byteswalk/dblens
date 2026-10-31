# M9b 账号与权限修改

依据：设计文档 `docs/superpowers/specs/2026-10-30-m9-privileges-design.md` 第 9 节。

## 结构

```text
crates/dblens-dialect/src/privilege/change.rs   按方言生成语句；PrivStatement 的 sql 只用于执行，masked 用于显示和落盘
crates/dblens-dialect/src/privilege/redact.rs   历史的尽力脱敏，不依赖 SQL 解析器
crates/dblens-service/src/privilege.rs          privilege_preview、privilege_apply：逐条执行，失败即停
crates/dblens-api/src/privileges.rs             POST /profiles/{id}/privileges/preview、/apply
web/src/features/privileges/PrivilegeDialogs.tsx  表单和统一的确认对话框
```

## 密码

- 只在请求体和发给数据库的语句里。`Secret` 的 `Debug` 输出 `***`，也不能序列化。
- 前端只放在组件状态里，对话框关闭即消失。随机密码在本地生成。
- `HistoryLog::record` 是所有历史的唯一入口，在这里脱敏，编辑器手写的语句也覆盖。
- 专用路径的错误先替换语句原文和密码，再脱敏。

## 策略（行为变化）

生产连接上的 `GRANT`、`REVOKE`、`CREATE`/`ALTER` 的 `USER`、`ROLE`、`LOGIN` 现在要确认，编辑器手写时也生效。
界面操作一律先预览；生产连接上删除主体、授予回收管理员权限要输入主体名，服务端也校验。

## 实测

`privilege_live` 的 `changes` 用哨兵密码走完建用户、建角色、授予、加入角色、改密、锁定、回收、移出、删除，再扫描数据目录确认没有哨兵。
通过：MySQL、MariaDB、TiDB、OceanBase、PostgreSQL、openGauss（两种模式）、金仓（两种模式）、Oracle、达梦、SQL Server 2022、SQL Server 2025 Express。

界面用真实鼠标在 dev 环境走过 MySQL 的生产连接：新建用户、生成密码、预览、锁定、状态刷新、输入主体名后删除、授权对话框。

## 发现的坑

- MySQL 的转授权单独存放，只 `REVOKE` 权限会留下 `USAGE`，要再 `REVOKE GRANT OPTION`。
- 达梦回收带转授权的权限要 `CASCADE`，否则报 -5582。
- 金仓对 `CREATE ROLE … LOGIN` 建的账号，`ALTER ROLE` 报“不是一个角色名”，统一用 `ALTER USER`。
- MariaDB 的 `JSON_VALUE` 对 JSON 布尔值返回 `1`，不是 `true`。M9a 的“已锁定”因此一直是否。
- openGauss 的锁定状态在 `pg_user_status`；OceanBase 没有成员的角色在 `role_edges` 里没有记录，要看无密码且锁定。
- `pg_user_status` 只有管理员能读，低权限账号走 `@limited` 或记入范围不完整。
- `UPDATE t SET password = 'x'` 不是账号语句，`SET PASSWORD` 只在语句开头才算。

## M9c GBase 8s

2026-10-31 补上修改：用户、角色、默认角色、数据库级和对象级授权、内部用户的改密和锁定。

- 账号有两种。操作系统用户：`sysusers` 只记录这个库里有权限的名字，新建不填密码时只 `GRANT CONNECT`，移除也只回收。内部用户：在 `sysuser` 库 `CREATE USER`，列为 `login`，只有它们能改密、锁定。
- 内部用户的语句先发 `DATABASE sysuser`，同一连接继续执行，连接只用这一次。要 DBSA（实测 `gbasedbt`）和实例开 `USERMAPPING`，否则报 -26703，界面原样显示。
- 回收要由授权人执行，别人回收写 `AS '授权人'`。不能按列回收，回收整张表的这项权限。数据库级权限不能转授。
- 角色要设为默认才在登录时生效：`GRANT DEFAULT ROLE`，界面在所属角色旁。
- `sysuser:sysintauthusers.flags` 第 1 位是锁定，读不到时走 `@limited`，按操作系统用户显示。
- `systabauth.tabauth` 共 9 位（s u * i d x a r n），大写表示可转授；`syscolauth.colauth` 共 3 位（s u r）。
- 系统表的字符列带尾随空格，比较和输出都要 `TRIM`。
- 角色要默认启用才生效，`defrole` 没设时路径标“需在会话中启用”。
- `privilege_live` 的 `gbase` 走 JDBC 桥接实连通过：两层角色路径、可转授、列权限、数据库级权限、属主和 DBA 的隐含权限。
