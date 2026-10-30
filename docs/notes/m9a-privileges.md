# M9a 数据库账号与权限（只读）

依据：设计文档 `docs/superpowers/specs/2026-10-30-m9-privileges-design.md`，实施计划 `docs/superpowers/plans/2026-10-30-m9a-privileges.md`。

## 结构

```text
crates/dblens-dialect/sql/privilege/*.sql      各方言模板，直接输出统一列名；段名带 @limited 的是低权限退路
crates/dblens-dialect/src/privilege/mod.rs     模型、模板加载、Dialect::privilege_query、public_grantees
crates/dblens-dialect/src/privilege/rows.rs    结果行解析、caps
crates/dblens-dialect/src/privilege/effective.rs  角色路径、实际权限、对象反查（纯函数）
crates/dblens-service/src/privilege.rs         取数、退级、Coverage
crates/dblens-service/tests/privilege_live.rs  实库测试（夹具建在测试库，结束时清理并断言）
crates/dblens-api/src/privileges.rs            三个 GET 接口
web/src/features/privileges/                   标签页和纯逻辑
```

## 接口

```text
GET /api/profiles/{id}/privileges?catalog=                       主体、成员关系、PUBLIC 主体、范围
GET /api/profiles/{id}/privileges/principal?principal=&catalog=  直接授权、实际权限、范围
GET /api/profiles/{id}/privileges/object?schema=&object=&catalog=  访问者、范围
```

## 查看范围

有两种“读不全”：

- 查询报 SQL 错误：退到 `@limited` 段，都失败时该段记入 `Coverage::Partial`。
- 查询成功但结果被数据库悄悄过滤：靠 `caps` 段的 `full_view` 判断。SQL Server 和 openGauss 属于这种。

实连测试有一条通用断言：报告范围完整时必须能看到夹具用户。openGauss 的问题就是这样发现的。

## 实测结果

规划阶段：模板在 MySQL 9.7.4、MariaDB 12.3.3、TiDB v8.5.8、OceanBase CE 4.4.2.1、PostgreSQL 18.6、金仓（两种模式）、openGauss（两种模式）、Oracle 23.26.3、SQL Server 2022 上用高权限账号逐段只读执行过。

`privilege_live` 完整流程（夹具、路径、反查、低权限连接）：

| 库 | 完整流程 | 低权限范围 |
|---|---|---|
| TiDB | 通过 | Partial（主体、成员关系） |
| OceanBase CE | 通过 | Partial（主体、成员关系） |
| openGauss（PG、A 模式） | 通过 | Partial（看不到别人） |
| 金仓（Oracle、PG 模式） | 通过 | Complete，能看到其他账号 |
| SQL Server 2022 | 通过 | Partial（看不到别人） |
| MySQL、MariaDB、PostgreSQL、Oracle | **未运行**，缺连接密码 | |
| 达梦 | **未运行**，模板从未实连 | |
| SQL Server 2025 | **未运行** | |

没设环境变量的测试会直接通过，`cargo test` 显示 ok 不代表实测过。

## 界面验证

用真实鼠标在浏览器模式下走过 TiDB 和 SQL Server：入口、列表筛选、属性和实际权限、对象反查、DENY 覆盖、跳转、标签恢复、低权限提示、英文、浅色和深色主题。

没有用鼠标验证的：SQL Server 切换数据库的下拉框、桌面程序窗口。“再次反查时不新开标签”只有单元测试。

## 发现的坑

- MySQL 的 `default_roles` 只记账号到角色这一层，嵌套角色的边不能判成“未默认启用”。
- MySQL 系连到某个库要求在库里有权限，什么权限都没有的账号连不上。
- openGauss 的 `CREATE ROLE` 必须带密码，用户带同名 schema，删除要 `CASCADE`。
- SQL Server 的登录名天然有 `CONNECT SQL`；登录名的连接开着时删不掉登录名，清理前要关掉连接。
- `pnpm` 测试和构建前要生成语法代码，直接跑 `tsc` 在新工作树里会报 `gen/` 缺失。
- 这次观察到对象树节点第一次右键不弹菜单，第二次才弹，涉及的右键逻辑 M9a 没有改动，未确认原因。
