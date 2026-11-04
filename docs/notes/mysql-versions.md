# MySQL 5.7、8.0

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第三项。原来只测过 9.7.4，按 8.0.13、8.0.16 写的版本分支没在老版本上跑过。

## 测试库

VM .106 上的 Docker 容器，镜像是华为云 SWR 同步的 Docker Hub 官方镜像。

| 版本 | 容器 | 端口 | 说明 |
|---|---|---|---|
| 5.7.44 | `my57` | 3317 | 5.7 的最后一版；默认字符集是 latin1，启动参数改成 utf8mb4 |
| 8.0.44 | `my80` | 3318 | |

- 账号 `admin`（全局全部权限，带 GRANT OPTION），库 `dblens_test`，密码同 9.7 的 `MYSQL_ADMIN`。
- 初始化脚本挂在 VM 的 `/var/lib/dblens-mysql-init/<容器>`，建完清空（容器重启时还要挂载这个目录）。
- 本机连接串 `DBLENS_TEST_MY57_URL`、`DBLENS_TEST_MY80_URL`，跑测试时赋给 `DBLENS_TEST_MYSQL_URL`。

## 改了什么（5.7）

| 问题 | 处理 |
|---|---|
| 5.7 只解析不保存检查约束，迁移过去读回少了约束 | 目标不执行检查约束时（`design_caps().checks` 为假：MySQL 8.0.16 以前、TiDB）不迁移检查约束，给出提示 |
| 5.7 接受 `DESC` 但按升序建索引 | 目标不支持降序索引时按升序迁移，给出提示 |
| 权限页查 `mysql.role_edges`、`default_roles`、`@@partial_revokes`，5.7 都没有 | 权限模板按版本分支：5.7 没有角色，成员关系为空；新增能力位 `roles`，5.7 上拒绝建角色、加入角色，界面隐藏这两个入口 |
| 保留字查 `information_schema.KEYWORDS`（8.0.13 才有） | 5.7 返回空，只用内置表。TiDB 报 8.0.11 但有这张表，单独覆盖 |

## 没解决：5.7 的 TLS

5.7.44 官方镜像用 OpenSSL 1.0.2 编译，ECDHE 握手失败，只能走 DHE 或 RSA 密钥交换；rustls 不支持这两种，`tls=require` 时报 `HandshakeFailure`。8.0 正常。用 openssl 在 VM 上验证过：ECDHE 套件被拒，DHE、RSA 套件可以连上。

## 测试

- 8.0.44：工作区全量通过，包括和 PostgreSQL、达梦之间的双向迁移。
- 5.7.44：除 TLS 的两项（驱动 TLS 测试、接口测试的加密连接）外全部通过。
- 夹具改成各版本通用的写法：`WITH RECURSIVE` 换成数字交叉连接，`CAST(… AS DOUBLE)` 换成 `1.5E0`，5.7 的元数据夹具不建函数索引和表达式默认值，权限测试在没有角色的库上走直接授权的流程。
