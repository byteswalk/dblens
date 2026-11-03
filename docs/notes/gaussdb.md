# GaussDB

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第五项。华为云下载页现在只提供混合云开发者版（集中式），用户确认后按它适配。分布式版和 DWS 没有免费安装包，不做。

测试库：VM .106 的容器 `gaussdb-dev`（Ubuntu 22.04，内核 507.0.T1），端口 8000，用户 `admin`。库 `dblens_test` 是 A 模式，`dblens_test_pg` 是 PG 模式。连接串 `DBLENS_TEST_GAUSSDB_URL`、`DBLENS_TEST_GAUSSDB_PG_URL` 在本机 `~/.dblens-test`。跑测试时把 A 模式的赋给 `DBLENS_TEST_OPENGAUSS_A_URL`，PG 模式的赋给 `DBLENS_TEST_OPENGAUSS_URL`：GaussDB 和 openGauss 用同一份查询，测试也共用。

容器起来后要手动启动数据库：用户 `omm`，`gs_ctl start -D /opt/gs/data`（`GAUSSHOME=/opt/gs/app`，`LD_LIBRARY_PATH` 指向它的 `lib`）。

## 改了什么

| 问题 | 处理 |
|---|---|
| A 模式没有识别成 Oracle 兼容（兼容模式只对金仓、openGauss 生效） | GaussDB 也读 `datcompatibility` |
| A 模式按 `nls_date_format`（默认 `DD-MON-RR`）解析日期，`DATE '2024-02-29'`、`'2026/1/2'::date` 都报错，导入失败 | 建连并识别方言后，A 模式会话执行 `SET nls_date_format = 'YYYY-MM-DD HH24:MI:SS'`（`Dialect::session_setup`） |
| A 模式的 DATE 实际是 `datea` 类型：查询结果显示成十六进制，迁移时当成不认识的类型转成文本 | 驱动按 timestamp 解码和绑定；逻辑类型同 Oracle 的 DATE（不带时区、秒精度） |
| 没有 `ON CONFLICT`，导入的“覆盖”模式语法错误 | 和 openGauss 一样用 `ON DUPLICATE KEY UPDATE` |
| 默认索引方法 `ubtree` 被原样迁到 PostgreSQL，建索引失败 | 迁移时 `btree`、`normal`、`ubtree` 都按默认方法处理，不写出来 |
| 保留字 `DBTIMEZONE`、`FENCED`、`PRIORER` 没有加引号 | 补进保留字表 |

## 测试

- 工作区全量测试在 A、PG 两种模式下都通过：驱动一致性、元数据、数据编辑、导入导出、表设计、权限（两层角色的成员关系能读到）、服务层和 API 全流程。
- 迁移：GaussDB 两种模式到 PostgreSQL、达梦都通过。到 MySQL、Oracle 的路线这次没起目标库，没有跑。
- 回归：openGauss（PG、A、SHA256 账号）、PostgreSQL、达梦全量通过。
- 测试夹具对 GaussDB 的差异：触发器写 `EXECUTE PROCEDURE`；不建表继承、域；表默认用 Ustore 存储，不支持 GIN 索引和物化视图，也不建。

## 限制

- GaussDB 默认 Ustore 表不支持 GIN 索引、物化视图，设计器和对象操作里不拦，由数据库报错。
- A 模式会话的日期格式被改成 ISO，用户在 SQL 编辑器里写 `TO_DATE('01-JAN-24')` 这类依赖默认格式的语句要自己写格式或改会话设置。
- 只测了开发者版这一个版本。下载页有新版时再换装复测。
