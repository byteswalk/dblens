# PostgreSQL 12、14

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第七项。原来只测过 18（VM 上 apt 装的，常开）。元数据模板按 10、11、12 写了版本分支，权限模板有 16 的分支。

## 测试库

VM .106 上的 Docker 容器，镜像是华为云 SWR 同步的 Docker Hub 官方镜像（`ddn-k8s/docker.io/postgres`）。

| 版本 | 容器 | 端口 | 说明 |
|---|---|---|---|
| 12.20 | `pg12` | 5412 | 镜像标签 `12`；12 的最后一版 12.22 镜像站没有 |
| 14.19 | `pg14` | 5414 | |

- 账号 `admin`（超级用户，同 18），库 `dblens_test`，密码同 18 的 `PG_ADMIN`。密码在建库时从挂载的文件读，建完清空（目录 `/var/lib/dblens-pg-init/<容器>` 要留着供重启时挂载）。
- 开了 SSL（数据目录里的自签名证书），和 18 一样；API 测试默认要求加密连接。
- 预加载并装了 `pg_stat_statements`，用来测运维页的慢 SQL（18 上没装）。
- 本机连接串 `DBLENS_TEST_PG12_URL`、`DBLENS_TEST_PG14_URL`，跑测试时赋给 `DBLENS_TEST_PG_URL`。

## 改了什么

| 问题 | 处理 |
|---|---|
| 慢 SQL 查 `pg_stat_statements` 的 `mean_exec_time` 等列，13 才改成这个名字（之前是 `mean_time`） | 用 `to_jsonb(s)` 按列名取，两种名字都认，不分版本 |
| 迁移到 PG 时 UUID 默认值写 `gen_random_uuid()`，13 起才内置（12 要装 pgcrypto） | 目标低于 13 时不迁移这个默认值，给出提示 |
| 同类问题：迁移到 MySQL 时日期、UUID 默认值写 `(CURRENT_DATE)`、`(UUID())`，表达式默认值 8.0.13 起才支持 | 目标是 8.0.13 以前的 MySQL 时同样不迁移，给出提示（当前时间仍写 `CURRENT_TIMESTAMP`） |
| 元数据夹具建了负小数位数的列 `numeric(5,-2)`，15 起才支持 | 夹具在 15 以前去掉这一列 |

## 测试

- 12.20、14.19：工作区全量通过（只配 PG、达梦连接串，含 PG 和达梦之间的双向迁移），包括加密连接、取消、元数据、设计器、权限、运维页（慢 SQL 实际查到了数据）。
- 18 回归：元数据、运维页通过（慢 SQL 照旧报扩展不存在，测试允许）。
