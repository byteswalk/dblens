# 达梦兼容模式和大小写不敏感

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第六项。

## 测试库

- `COMPATIBLE_MODE`：用 VM .106 上独立的 5237 实例（`/dmdata/jdbc`，账号 `DBLENS_JDBC`）依次切到 0、2、3、4、7，各跑一遍工作区全量测试，测完改回 0 并停掉。连接串 `DBLENS_TEST_DM5237_URL`。
- 大小写不敏感：另建了 5238 实例（`/dmdata/ci`，`CASE_SENSITIVE=N`，账号 `DBLENS_TEST`，密码是 creds.env 的 `DM_CI`），连接串 `DBLENS_TEST_DM_CI_URL`。平时停着，启动方法和 5237 一样，`dm.ini` 换成 `/dmdata/ci/DAMENG/dm.ini`。
- 跑的时候把上面的连接串赋给 `DBLENS_TEST_DM_URL`。

## 结果

| 模式 | 结果 |
|---|---|
| 0 不兼容 | 通过 |
| 2 Oracle | 原先只有迁移读回结构不一致，已修，见下 |
| 3 SQL Server | 通过 |
| 4 MySQL | 通过 |
| 7 PG | 通过 |
| `CASE_SENSITIVE=N` | 产品功能都通过；改的是测试 |

1、5、6、8（SQL92、DM6、Teradata、DB2）现场少见，没有测。

## 模式 2（Oracle 兼容）

这个模式下达梦的行为和 Oracle 一样：空串存成 NULL，`DATE` 带时分秒，建表时直接变成 `TIMESTAMP(0)`。以前迁移到这种库时，规划里写的是 `DATE`，建出来是 `TIMESTAMP(0)`，读回结构和规划对不上。

改法：连接时用 `SF_GET_PARA_VALUE(1, 'COMPATIBLE_MODE')` 探测兼容模式，2 按 Oracle 兼容处理（`Dialect::oracle_mode()` 为真），于是：

- 数据编辑把空串当 NULL，和数据库实际存的一致。
- 迁移和设计里的日期类型直接写 `TIMESTAMP(0)`。

其他模式按原生处理，连接时不提示“未适配的模式”：3、4、7 下我们生成的 SQL 都能正常执行。

低权限账号没有读 INI 参数的权限（-5730）。为此把兼容模式探测改成失败不挡连接：记一条警告，按原生模式处理。所有带探测的库（金仓、openGauss、GaussDB、PolarDB）都是这样。

## 大小写不敏感（`CASE_SENSITIVE=N`）

这种库按书写的原样存标识符（`CREATE TABLE zz_Ci (Id INT)` 存成 `zz_Ci`、`Id`），比较时不分大小写，带引号的名字也一样。界面上用的是元数据里读到的名字，所以产品不用改。

失败的是测试：测试假定达梦总把不带引号的名字转成大写，用大写名字去找对象。现在测试连接时用 `SF_GET_CASE_SENSITIVE_FLAG()` 判断，不敏感的库按书写原样找。

## 其他

- 元数据测试原先写死 `DBLENS_TEST` schema，换成 5237 的 `DBLENS_JDBC` 账号就找不到表。改成按连接账号取 schema。

## 没有验证的

- 用户的真实达梦库（DM8 0x7000c）上 `SF_GET_PARA_VALUE` 能不能调用、权限够不够没有实测。即使调用失败也只会按原生模式处理，不影响连接。
