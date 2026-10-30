# DBLens（数洞）

跨平台数据库管理工具，后端 Rust，前端 Web UI，形态是 Tauri 桌面应用加可选的浏览器模式。

- 设计文档：`docs/superpowers/specs/2026-10-22-dblens-design.md`
- 实施计划：`docs/superpowers/plans/`

## 开发

```bash
cargo build --workspace
cargo test --workspace
```

## 集成测试数据库

驱动的一致性测试需要真实数据库。连接 URL 从环境变量读取，未设置时对应测试会自动跳过：

| 环境变量 | 示例 |
|---|---|
| `DBLENS_TEST_MYSQL_URL` | `mysql://root:dblens@127.0.0.1:3306/dblens_test` |
| `DBLENS_TEST_MARIADB_URL` | `mysql://root:dblens@127.0.0.1:3307/dblens_test` |
| `DBLENS_TEST_PG_URL` | `postgres://postgres:dblens@127.0.0.1:5432/dblens_test` |
| `DBLENS_TEST_DM_URL` | `dm://SYSDBA:密码@127.0.0.1:5236` |
| `DBLENS_TEST_ORACLE_URL` | `oracle://dblens:dblens@127.0.0.1:1521/FREEPDB1` |
| `DBLENS_TEST_SQLSERVER_URL` | `sqlserver://sa:密码@127.0.0.1:1433/dblens_test` |

- 达梦和 Oracle 驱动在运行时加载本机的客户端库：达梦需要安装达梦客户端（dmdpi），Oracle 需要 Instant Client。库所在目录可以用 URL 参数 `dpi_lib_dir` / `oracle_client_lib_dir` 指定，不指定时会按 `DM_HOME` / `ORACLE_HOME`、PATH 和常见安装位置自动查找。
- 达梦集成测试只能在本地运行（CI 中没有达梦镜像）；达梦 DPI 技术验证：`cargo test -p dblens-driver-dameng spike -- --ignored --nocapture --test-threads=1`。

有 Docker 时可以一键启动这三个测试库：

```bash
docker compose -f docker/docker-compose.test.yml up -d
```

PowerShell 中设置环境变量：`$env:DBLENS_TEST_MYSQL_URL = "mysql://root:dblens@127.0.0.1:3306/dblens_test"`
