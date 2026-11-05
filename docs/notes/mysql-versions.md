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

## TLS 改用系统实现

5.7.44 官方镜像用 OpenSSL 1.0.2 编译，ECDHE 握手失败，只能走 DHE 或 RSA 密钥交换（在 VM 上用 openssl 验证过：ECDHE 套件被拒，DHE、RSA 套件能连上）。rustls 不支持这两种，`tls=require` 时报 `HandshakeFailure`。

2026-11-05 起 MySQL 驱动（mysql_async）的 TLS 从 rustls 换成 native-tls：

- Windows 用 SChannel，macOS 用 Security.framework，Linux 上把 OpenSSL（openssl-src 3.6）静态编进去，不依赖系统的 libssl。
- `require`、`verify`、`tls_ca` 的含义不变。PG、SQL Server 驱动仍用 rustls。
- openssl、openssl-sys 用 0.10.81、0.9.117：更新的版本发布不满 7 天。openssl-src 停在 3.x、pem 停在 3.x 是上游依赖的版本约束。
- Windows 上已验证：5.7、8.0、9.7、MariaDB 的加密连接、证书校验失败、加密连接上的取消都通过。
- Linux 构建（2026-11-05，在 VM .106 的 `rust:1.99-bookworm` 容器里，依赖用 `cargo vendor` 带过去）：
  - 原生编译和 cargo-zigbuild（v0.24.0，zig 0.17.0）按 glibc 2.28 交叉编译 x64、ARM64 都能编出来。静态编入 OpenSSL 要求构建环境有 perl 和 make，这个镜像自带。
  - 二进制不链接系统的 libssl、libcrypto，最高引用 GLIBC_2.28，ARM64 版依赖的只有 libc、libm、libpthread、libdl。
  - x64 版（原生和 glibc 2.28 交叉编译的）在 Linux 上连 MySQL 5.7、8.0 的 TLS 测试和驱动一致性测试都通过。ARM64 版没有运行（VM 上没有 qemu）。

## 测试

- 8.0.44：工作区全量通过，包括和 PostgreSQL、达梦之间的双向迁移。
- 5.7.44：全量通过，包括加密连接（换成 native-tls 之后）。
- 夹具改成各版本通用的写法：`WITH RECURSIVE` 换成数字交叉连接，`CAST(… AS DOUBLE)` 换成 `1.5E0`，5.7 的元数据夹具不建函数索引和表达式默认值，权限测试在没有角色的库上走直接授权的流程。
