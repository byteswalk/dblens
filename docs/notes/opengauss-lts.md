# openGauss LTS 5.0、6.0、7.0

覆盖计划（`docs/superpowers/specs/2026-11-02-db-coverage-plan.md`）的第四项。原来只测过 7.0.0-RC1。

## 测试库

VM .106 上每个版本一个容器，都有 PG 兼容库 `dblens_test`、A 兼容库 `dblens_test_a`，以及只存 SHA256 密码的账号 `admin_sha` 和它的库 `dblens_test_sha`。远程认证都是 sha256。

| 版本 | 容器 | 端口 | 安装方式 |
|---|---|---|---|
| 7.0.0 LTS | `og7` | 5434 | 官网轻量版安装包，装在 7.0.0-RC1 镜像（openEuler 20.03）里 |
| 6.0.6 LTS | `og6` | 5435 | 同上 |
| 5.0.0 LTS | `og5` | 5436 | 华为 SWR 上的官方镜像 `opengauss/opengauss:5.0.0` |

- 轻量版安装包来自 `opengauss.obs.cn-south-1.myhuaweicloud.com/<版本>/openEuler20.03/x86/`，已核对 sha256。
- 轻量版依赖 `libatomic`、`openblas`，RC1 镜像里没有，要先在容器里 `dnf install`。
- 5.0 的轻量包命名不同，没找到，所以用镜像；Docker Hub 和 daocloud 镜像站都拉不到 6.0、7.0 的镜像。
- 本机连接串是 `DBLENS_TEST_OG{7,6,5}_URL`、`_A_URL`、`_SHA_URL`。跑测试时赋给 `DBLENS_TEST_OPENGAUSS_URL`、`_A_URL`、`_SHA_URL`。

## 改了什么

| 问题 | 版本 | 处理 |
|---|---|---|
| 序列的 `relkind` 变成 `'z'`，元数据列不出序列，迁移也带不过去 | 7.0 | 序列和权限查询认 `'S'`、`'L'`、`'z'` |
| 新保留字 `SHARE_MEMORY` 没有加引号 | 7.0 | 补进保留字表 |
| `charset` 不能当不加引号的列别名，规范检查和设计器读表存储信息报语法错误 | 5.0 | PG 系查询里给 `charset`、`collation` 别名加引号 |
| 出错位置按字节报（6.0 起按字符），带中文的语句出错时编辑器定位偏后 | 5.0 | 从版本串识别 openGauss 主版本，5 及以前按字节换算 |

## 服务端的问题（记为限制）

- 5.0 的 numeric NaN 走二进制协议时发成全零，客户端读到 0，分不出来。驱动一致性测试对 openGauss 去掉了 NaN 用例。
- 5.0 不支持表继承，6.0 及以前的触发器不认 `EXECUTE FUNCTION`。这两点只影响测试夹具：夹具改写 `EXECUTE PROCEDURE`，5.0 不建继承表。

## 测试

- 三个版本的 PG、A 两种兼容模式和 SHA256 账号，工作区全量测试都通过，包括和 PostgreSQL、达梦之间的双向迁移。
- 回归：PostgreSQL、达梦全量通过。
