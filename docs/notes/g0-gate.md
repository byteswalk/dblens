# G0 公开仓库改造

依据：设计文档 `docs/superpowers/specs/2026-10-31-audit-edition-design.md` 第 2、3.1 节；计划 `docs/superpowers/plans/2026-10-31-g0-gate-extension-points.md`。

单机版行为不变。这里放开的扩展点由私有仓库的 `dblens-gate` 使用。

## 结构

```text
crates/dblens-service/src/operator.rs     Operator：操作者，task-local 传递
crates/dblens-service/src/policy.rs       Operation、ExecContext::new、RequireTicket、ask、report
crates/dblens-service/src/source.rs       CredentialSource、ProfileSource
crates/dblens-service/src/path_tests.rs   各执行路径和策略的衔接，操作者隔离
crates/dblens-service/tests/architecture.rs  入口约束
crates/dblens-api/src/lib.rs              api_routes：不带鉴权层的业务路由
web/src/index.ts                          前端包入口
web/src/shared/capabilities.ts            版本和能力开关
```

## 四个扩展点

| 扩展点 | 单机版 | 审计版怎么用 |
|---|---|---|
| `Policy` | `DefaultPolicy` | 实现规则判定，`after_execute` 写审计 |
| `CredentialSource` | `Secrets` | 解密托管凭据 |
| `ProfileSource` | `Store` | 内存快照，定时从元库刷新 |
| `api_routes()` | `router()` 加令牌鉴权 | 包自己的鉴权层，在层里用 `Operator::scope` 设定操作者，自己实现 `/capabilities` |

`Service::with_sources` 把后三者组装起来。

## 操作者

`Operator::current()` 没有设定时是本机用户，所以单机版不用改任何调用。

task-local 不会跟着 `tokio::spawn` 走：
- 后台任务的 `ExecContext` 在 spawn 之前建好；
- 会话在打开时记下操作者，之后都用这个；
- `ws.rs` 里 `on_upgrade` 和 `pump` 显式带过去。漏掉时用户会拿到自己会话的 `NotFound`，`websocket_keeps_the_operator` 测试的就是这一点。

别人的会话、任务、导出、上传一律返回 `NotFound`，不暴露是否存在。

## 判定和回报

- `policy::ask` 是调用 `before_execute` 的唯一入口。架构测试检查这一点，也检查 `.secrets.get(` 不再出现，并列出全部建连的文件。
- 逐条回报：编辑器、数据编辑、表设计器、同步、SQL 文件、迁移 DDL、导出、权限修改。
- 批量写入只回报一条 `-- ` 开头的摘要：导入、迁移每张表的数据复制。否则审计量失控。
- 迁移里的会话级开关（`identity_insert`、切换 schema）不回报。
- 权限修改只把脱敏后的语句交给策略。

## 和规格的差别

1. 数据编辑仍把 `Confirm` 当已确认，因为预览就是确认。审计版的策略只返回 `RequireTicket`，效果等同于规格。`confirmed` 标志同理。
2. `RequireTicket` 目前按拒绝处理（`Error::Denied`）。“提交工单”的交互在 G2。
3. `ProfileSource` 只有读操作，且是同步接口。
4. **迁移读源库不判定，G2 必须补上**，否则“任何入口都绕不过审批”不成立：现在用迁移能绕开导出审批。
5. `Operation` 比规格的七类多了 `Design`、`Compare`、`SqlFile`。

## 没有覆盖的

- 迁移的回报没有单独的测试。`Ctx::report` 是一行转发，表设计器路径的测试覆盖了 `Run::sql`。
- 比对同步的回报同样走 `Run::sql`，没有单独用例。
- 前端只放了能力开关，还没有地方使用，G1 再接。
- 前端的 `?token=` 和 `Authorization` 头在审计版用 Cookie 时要去掉或忽略，G1 处理。

## 验证状态

- 通过：工作区离线测试（不含桌面端测试）、clippy、前端 341 个测试和类型检查、`dblens-api` 和 `dblens-service` 的 `live` 实连测试。
- 界面冒烟（真实鼠标）：编辑器执行、生产环境 DROP 提示需要确认、打开导出对话框。
- **未做，用户决定放到后面**：`migrate_live`、`privilege_live` 实连测试；界面上的数据编辑保存、表设计器执行、导出下载、SQL 文件导入、账号权限修改。

## 构建

磁盘紧张时用 `CARGO_INCREMENTAL=0`、`CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`，target 目录从 11G 降到不足 3G。
