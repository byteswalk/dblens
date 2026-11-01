# G0 公开仓库改造 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在公开仓库放开审计版需要的扩展点，并让所有执行路径都经过同一个判定入口，单机版行为不变。

**Architecture:** 操作者用 task-local 传递（同现有 `scope_lang`），不改各服务方法的签名。`policy::ask` 成为唯一调用 `before_execute` 的地方；执行后的 `after_execute` 覆盖全部写路径。凭据和连接配置改由 trait 提供，API 路由可不带鉴权层组合。架构测试锁住这些入口。

**Tech Stack:** Rust（tokio、axum、async-trait）、React 19、pnpm workspace 包导出。

**Spec:** `docs/superpowers/specs/2026-10-31-audit-edition-design.md`（第 2、3.1 节，第 12 节 G0）

## Global Constraints

- 单机版行为不变：现有全部测试不改断言即通过（只允许机械地改函数调用形式）。
- 不打包、不发布、不改 `DefaultPolicy` 的判定结果。
- 依赖用最新稳定版且发布满 7 天；本计划预计不新增 Rust 依赖。
- 用户可见文字走 `msg!` 和 `crates/dblens-core/locales/app.json`，中英文都要有。
- 代码注释用中文，风格同周围代码。
- 提交身份 ThinkBuddy；推送要用户同意。
- 权限修改路径只把脱敏后的语句交给策略，密码绝不进入 `Policy`。

## 规格细化（实现时以此为准）

1. **数据编辑和 `Confirm`**：规格写“不再忽略 `Confirm`”。单机版里预览就是确认，若改成阻塞会改变行为。所以 `apply` 继续把 `Confirm` 当已确认，但要处理新增的 `RequireTicket`（失败）。审计版的策略不返回 `Confirm`，只返回 `RequireTicket`，因此效果等同于规格要求。`confirmed` 标志同理。
2. **`RequireTicket` 在 G0 的处理**：所有路径按拒绝处理（`Error::Denied`），判不清就不放行。G2 再加“提交工单”的交互。
3. **`ProfileSource` 只含读操作，且是同步接口**：连接的增删改只在单机版路由里用，审计版不挂载这些路由。审计版用内存快照实现。
4. **迁移读源库不判定**：G0 只判定目标库写入。从源库读数据相当于导出，G2 再加入，避免 G0 引入新语义。
5. **`Operation` 在规格 3.1 的七类之外细分** `Design`、`Compare`、`SqlFile`，审批规则需要区分它们。

## 文件结构

| 文件 | 职责 |
|---|---|
| `crates/dblens-service/src/operator.rs`（新建） | `Operator` 和 task-local |
| `crates/dblens-service/src/policy.rs` | `Operation`、`ExecContext::new`、`RequireTicket`、`ask`、`Policy` 新签名 |
| `crates/dblens-service/src/source.rs`（新建） | `CredentialSource`、`ProfileSource` 及单机版实现 |
| `crates/dblens-service/src/{session,design,compare,sqlfile,privilege}.rs`、`import/job.rs`、`export/job.rs`、`migrate/{mod,run,copy}.rs` | 接入 `ask`、`after_execute`、凭据 |
| `crates/dblens-service/src/task.rs` | 任务、上传归属 |
| `crates/dblens-service/tests/architecture.rs`（新建） | 入口约束 |
| `crates/dblens-api/src/lib.rs`、`ws.rs` | 路由组合、`/capabilities`、操作者传递 |
| `web/src/index.ts`、`web/src/shared/capabilities.ts`（新建）、`web/package.json` | 前端包入口和能力开关 |
| `docs/notes/g0-gate.md`（新建） | 实现说明 |

---

### Task 1: 操作者、操作类型和判定入口

**Files:**
- Create: `crates/dblens-service/src/operator.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/policy.rs:54-87,104-128`
- Modify: `crates/dblens-service/src/session.rs:289-295,608-632,776-787,875-879`
- Modify: `crates/dblens-service/src/design.rs:336-360`、`compare.rs:369`、`migrate/mod.rs:312`
- Modify: `crates/dblens-service/src/sqlfile.rs:150-156,200-229`、`import/job.rs:327-340`、`privilege.rs:334-342,367`
- Modify: `crates/dblens-core/locales/app.json`
- Test: `crates/dblens-service/src/policy.rs`、`crates/dblens-service/src/operator.rs`

**Interfaces:**
- Produces:
  - `Operator { id: String, name: String }`，`Operator::LOCAL_ID`、`Operator::local()`、`Operator::current() -> Operator`、`Operator::scope(op, fut)`
  - `Operation`：`Editor | DataEdit | Design | Compare | Import | SqlFile | Export | Migrate | Privilege`
  - `ExecContext { profile_id, environment, dialect, operator, operation }`，`ExecContext::new(&Profile, Dialect, Operation)`
  - `Decision::RequireTicket { reason: Msg }`，`Decision::blocked(self) -> Option<Msg>`（`Deny`、`RequireTicket` 返回原因）
  - `Policy::before_execute(&self, ctx, stmt, sql: &str) -> Decision`
  - `Policy::after_execute(&self, ctx, stmt, sql: &str, outcome)`
  - `policy::ask(policy: &Arc<dyn Policy>, ctx: &ExecContext, sql: &str) -> (StatementInfo, Decision)`

- [ ] **Step 1: 写失败的测试**

在 `operator.rs` 末尾：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_defaults_to_local_and_follows_scope() {
        assert_eq!(Operator::current().id, Operator::LOCAL_ID);
        let op = Operator { id: "u1".into(), name: "张三".into() };
        let seen = Operator::scope(op.clone(), async { Operator::current() }).await;
        assert_eq!(seen, op);
        assert_eq!(Operator::current().id, Operator::LOCAL_ID);
    }
}
```

在 `policy.rs` 的测试模块中：

```rust
    struct Spy(std::sync::Mutex<Vec<(String, String, Operation)>>);

    #[async_trait]
    impl Policy for Spy {
        async fn before_execute(&self, ctx: &ExecContext, _: &StatementInfo, sql: &str) -> Decision {
            self.0.lock().unwrap().push((ctx.operator.id.clone(), sql.to_string(), ctx.operation));
            Decision::RequireTicket { reason: msg!("policy.ticket_required") }
        }
        async fn after_execute(&self, _: &ExecContext, _: &StatementInfo, _: &str, _: &Outcome) {}
    }

    #[tokio::test]
    async fn ask_passes_full_text_operator_and_operation() {
        let spy = Arc::new(Spy(Default::default()));
        let policy: Arc<dyn Policy> = spy.clone();
        let ctx = ExecContext { operation: Operation::Export, ..ctx(Environment::Prod) };
        let (info, d) = ask(&policy, &ctx, "SELECT 1").await;
        assert_eq!(info.keyword, "SELECT");
        assert!(d.blocked().is_some());
        assert_eq!(spy.0.lock().unwrap()[0], ("local".into(), "SELECT 1".into(), Operation::Export));
    }
```

测试里的 `ctx()` 辅助函数改为补上 `operator: Operator::local(), operation: Operation::Editor`，其余 `ExecContext { .. }` 字面量同样补字段；调用 `before_execute` 的测试补第三个参数 `sql`。

- [ ] **Step 2: 确认失败**

Run: `cargo test -p dblens-service --lib policy`
Expected: 编译失败，找不到 `Operator`、`ask`。

- [ ] **Step 3: 实现 `operator.rs`**

```rust
//! 操作者（审计版设计 3.1 节）：谁在执行。单机版固定为本机用户；审计版由鉴权层用 `Operator::scope` 设定。
//! 用 task-local 传递，后台任务要在 `tokio::spawn` 之前取出并自己保存。

use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    pub id: String,
    pub name: String,
}

tokio::task_local! {
    static CURRENT: Operator;
}

impl Operator {
    pub const LOCAL_ID: &'static str = "local";

    pub fn local() -> Self {
        Self { id: Self::LOCAL_ID.into(), name: String::new() }
    }

    /// 当前任务的操作者，没有设定时是本机用户。
    pub fn current() -> Self {
        CURRENT.try_with(Clone::clone).unwrap_or_else(|_| Self::local())
    }

    pub async fn scope<F: Future>(op: Operator, fut: F) -> F::Output {
        CURRENT.scope(op, fut).await
    }
}
```

`lib.rs` 增加 `mod operator;` 和 `pub use operator::Operator;`（`policy` 的新类型按现有 `pub use` 方式导出）。

- [ ] **Step 4: 改 `policy.rs`**

```rust
/// 语句来自哪个功能：审批规则按它区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Editor,
    DataEdit,
    Design,
    Compare,
    Import,
    SqlFile,
    Export,
    Migrate,
    Privilege,
}

/// 执行上下文：谁、用什么功能、在哪个连接上执行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecContext {
    pub profile_id: String,
    pub environment: Environment,
    pub dialect: Dialect,
    pub operator: Operator,
    pub operation: Operation,
}

impl ExecContext {
    /// 操作者取自当前任务：后台任务要在 spawn 之前建好。
    pub fn new(profile: &Profile, dialect: Dialect, operation: Operation) -> Self {
        Self {
            profile_id: profile.id.clone(),
            environment: profile.input.environment,
            dialect,
            operator: Operator::current(),
            operation,
        }
    }
}
```

`Decision` 增加：

```rust
    /// 要走工单，当前不能执行（审计版）。
    RequireTicket {
        reason: Msg,
    },
```

```rust
impl Decision {
    /// 不能执行时的原因。
    pub fn blocked(self) -> Option<Msg> {
        match self {
            Decision::Deny { reason } | Decision::RequireTicket { reason } => Some(reason),
            Decision::Allow | Decision::Confirm { .. } => None,
        }
    }
}

/// 分析一条语句并询问策略。所有执行路径都从这里进，不直接调用 `before_execute`。
pub(crate) async fn ask(
    policy: &Arc<dyn Policy>,
    ctx: &ExecContext,
    sql: &str,
) -> (StatementInfo, Decision) {
    let info = classify(&ctx.dialect, sql);
    let decision = policy.before_execute(ctx, &info, sql).await;
    (info, decision)
}
```

`Policy` 两个方法加 `sql: &str`。`DefaultPolicy` 忽略该参数，判定逻辑不变。

`app.json` 增加 `policy.ticket_required`：`"zh-CN": "这条语句需要走工单审批"`，`"en": "This statement requires an approved ticket"`（格式照文件内已有词条）。

- [ ] **Step 5: 改全部调用方**

`session.rs`：
- 289 行的 `ctx` 改为 `ExecContext::new(profile, dialect, Operation::Editor)`。
- `apply`（608 行）用同一会话但属于数据编辑：`let ctx = &ExecContext { operation: Operation::DataEdit, ..ctx.clone() };`，放在循环前。
- 循环内：

```rust
            let (info, decision) = ask(policy, ctx, &st.sql).await;
            let result: Result<u64> = async {
                if let Some(reason) = decision.blocked() {
                    return Err(Error::Denied { reason });
                }
```

  并把 632 行改成 `policy.after_execute(ctx, &info, &st.sql, &outcome).await;`。
- 776–808 行：用 `let (stmt, decision) = ask(policy, ctx, &step.texts[0]).await;`，`match decision`，把 `Decision::Deny { reason }` 分支改为 `Decision::Deny { reason } | Decision::RequireTicket { reason }`。
- 879 行补 `&step.texts[0]`。

`design.rs:336` 的 `check_policy` 增加 `operation: Operation` 参数：

```rust
        let ctx = ExecContext::new(profile, dialect, operation);
        let policy = self.sessions.policy();
        let (mut denied, mut confirm) = (None, Vec::new());
        for sql in sqls {
            match ask(&policy, &ctx, sql).await.1 {
                Decision::Allow => {}
                Decision::Confirm { reason } => {
                    if !confirm.contains(&reason) {
                        confirm.push(reason);
                    }
                }
                Decision::Deny { reason } | Decision::RequireTicket { reason } => {
                    denied.get_or_insert(reason);
                }
            }
        }
```

调用处：`design.rs:329` 传 `Operation::Design`，`compare.rs:369` 传 `Operation::Compare`，`migrate/mod.rs:312` 传 `Operation::Migrate`。

`sqlfile.rs`：`Run` 增加 `ctx: ExecContext`，在 `start_sql_file` 里 spawn 之前用 `ExecContext::new(&profile, dialect, Operation::SqlFile)` 建好；`scan_sql_file` 去掉本地 `ctx`，215 行改 `match ask(&policy, &run.ctx, &st.text).await.1`，`Decision::Deny` 分支同样加上 `RequireTicket`。

`import/job.rs:327`：`ExecContext::new(&profile, dialect, Operation::Import)`；337 行：

```rust
            if let Some(reason) = ask(&policy, &ctx, sql).await.1.blocked() {
                return Err(Error::Denied { reason });
            }
```

`privilege.rs:334`：`ExecContext::new(&profile, dialect, Operation::Privilege)`；用脱敏语句询问，顺序和现在一样，先全部判定后执行：

```rust
        let mut infos = Vec::with_capacity(statements.len());
        for s in &statements {
            let (info, decision) = ask(&policy, &ctx, &s.masked).await;
            if let Some(reason) = decision.blocked() {
                return Err(Error::Denied { reason });
            }
            infos.push(info);
        }
```

367 行改 `policy.after_execute(&ctx, info, &statement.masked, &summary).await;`。

- [ ] **Step 6: 跑测试**

Run: `cargo test -p dblens-service --lib`
Expected: 全部 PASS。原有断言一条不改。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-service crates/dblens-core/locales/app.json
git commit -m "feat(policy): 策略带上操作者、操作类型和完整语句，统一判定入口"
```

### Task 2: 导出接入策略，写路径全部回报结果

**Files:**
- Modify: `crates/dblens-service/src/export/job.rs:225-244,288-340`
- Modify: `crates/dblens-service/src/design.rs:413-443`
- Modify: `crates/dblens-service/src/sqlfile.rs:285-296`
- Modify: `crates/dblens-service/src/import/job.rs`（任务结束处）
- Modify: `crates/dblens-service/src/migrate/run.rs:237,358,374`、`migrate/copy.rs:59,85,128,156`
- Test: 上述文件的测试模块

**Interfaces:**
- Consumes: Task 1 的 `ask`、`ExecContext`、`Outcome`
- Produces: `policy::report(policy: &Arc<dyn Policy>, ctx: &ExecContext, sql: &str, result: &Result<u64>)`，把结果转成 `Outcome` 后调用 `after_execute`

规则：
- 执行者提供的 SQL（表设计器、同步、SQL 文件、迁移 DDL、导出）逐条回报。
- 批量数据写入（导入、迁移数据复制 `copy.rs`）不逐批回报，任务结束时用一条摘要语句回报总行数，否则大批量时审计量失控。
- `copy.rs:59` 的 `DELETE FROM` 和 `copy.rs:85` 的结构语句属于逐条回报范围，128、156 行是数据批次，不回报。

- [ ] **Step 1: 写失败的测试**

在 `export/job.rs` 测试模块加：

```rust
    struct DenyExport;

    #[async_trait::async_trait]
    impl Policy for DenyExport {
        async fn before_execute(&self, ctx: &ExecContext, _: &StatementInfo, _: &str) -> Decision {
            if ctx.operation == Operation::Export {
                Decision::RequireTicket { reason: dblens_core::msg!("policy.ticket_required") }
            } else {
                Decision::Allow
            }
        }
        async fn after_execute(&self, _: &ExecContext, _: &StatementInfo, _: &str, _: &Outcome) {}
    }

    #[tokio::test]
    async fn policy_can_block_export() {
        let (s, id, _log, _dir) = service_with_policy(Arc::new(DenyExport));
        let r = s.start_export(request(&id, "ROWS 1", ExportFormat::Csv), false).await;
        assert!(matches!(r, Err(Error::Denied { .. })));
    }
```

把现有 `service()` 拆成 `service_with_policy(policy: Arc<dyn Policy>)`，`service()` 调用它并传 `Arc::new(DefaultPolicy)`。

在 `design.rs` 测试模块加一个记录 `after_execute` 的 `Spy` 策略，断言一次表设计执行后，`Spy` 收到的语句和执行的语句一一对应、`Operation::Design`：

```rust
    #[derive(Default)]
    struct Spy(std::sync::Mutex<Vec<(String, Operation)>>);

    #[async_trait::async_trait]
    impl Policy for Spy {
        async fn before_execute(&self, _: &ExecContext, _: &StatementInfo, _: &str) -> Decision {
            Decision::Allow
        }
        async fn after_execute(&self, ctx: &ExecContext, _: &StatementInfo, sql: &str, _: &Outcome) {
            self.0.lock().unwrap().push((sql.to_string(), ctx.operation));
        }
    }
```

断言写法沿用该模块现有的执行测试（723 行附近），改用 `Spy` 构造 `Service`，执行后：

```rust
        let seen = spy.0.lock().unwrap();
        assert!(!seen.is_empty());
        assert!(seen.iter().all(|(_, op)| *op == Operation::Design));
```

`sqlfile.rs` 和 `migrate/run.rs` 各加一个同样形式的测试（`Operation::SqlFile`、`Operation::Migrate`），分别复用该模块现有的执行用例。`Spy` 放到 `crate::fake` 里共用，不要复制三份。

- [ ] **Step 2: 确认失败**

Run: `cargo test -p dblens-service --lib policy_can_block_export`
Expected: FAIL，当前导出不询问策略。

- [ ] **Step 3: 实现**

`policy.rs` 增加（`Task 1` 已加入 `use std::sync::Arc`、`Operator`、`Profile`；这里再引入 `dblens_core::{Error, Result}`）：

```rust
/// 把一条语句的执行结果告诉策略。
pub(crate) async fn report(
    policy: &Arc<dyn Policy>,
    ctx: &ExecContext,
    sql: &str,
    result: &Result<u64>,
) {
    let outcome = match result {
        Ok(n) => Outcome::Succeeded { affected: Some(*n) },
        Err(Error::Cancelled) => Outcome::Cancelled,
        Err(e) => Outcome::Failed { message: e.to_string() },
    };
    policy.after_execute(ctx, &classify(&ctx.dialect, sql), sql, &outcome).await;
}
```

`export/job.rs`：在 244 行 `sql` 算好之后：

```rust
        // 单条查询的预览就是用户的确认，这里只处理不能执行的情况
        let ctx = ExecContext::new(&profile, dialect, Operation::Export);
        let policy = self.sessions.policy();
        if let Some(reason) = ask(&policy, &ctx, &sql).await.1.blocked() {
            return Err(Error::Denied { reason });
        }
```

任务结束写 `history.record` 处（331 行）之前：`report(&policy, &ctx, &sql, &result.as_ref().map(|_| rows.load(Ordering::SeqCst)).map_err(Clone::clone)).await;`（`result` 的具体类型以该处为准，`Ok` 里取导出行数）。

`design.rs`：`Run` 增加 `pub ctx: ExecContext`，在 `start_design`、`compare.rs` 的同步、`migrate` 构造 `Run` 处 spawn 之前用 `ExecContext::new` 建好（`Operation` 分别是 `Design`、`Compare`、`Migrate`）。`Run::sql` 改为：

```rust
    pub(crate) async fn sql(&self, conn: &mut Box<dyn Connection>, sql: &str) -> Result<u64> {
        let (at, started) = (now_ms(), Instant::now());
        let result = exec(conn, sql, &[]).await;
        self.record(sql, at, started, result.as_ref().ok().copied(), result.as_ref().err());
        report(&self.service.sessions.policy(), &self.ctx, sql, &result).await;
        result
    }
```

`Run::execute` 中重建表的 `Part::Rebuild` 分支若有绕过 `self.sql` 直接 `exec` 改数据的语句，一并改走 `self.sql`，`schema_switch_sql` 和只读计数除外。

`sqlfile.rs:288`：

```rust
                let result = exec(&mut conn, &st.text, &[]).await;
                report(&policy, &run.ctx, &st.text, &result).await;
                match result {
```

`let policy = self.sessions.policy();` 放在循环前。

`migrate/run.rs:358,374`、`copy.rs:59,85`：改为先 `exec` 取结果，再 `report`，再按原逻辑处理。`copy.rs` 的函数没有上下文时加 `gate: (&Arc<dyn Policy>, &ExecContext)` 参数。

`import/job.rs`：任务结束写历史处（383 行）之前调用：

```rust
        report(&policy, &ctx, &format!("-- import {} rows into {}", rows, table), &result).await;
```

`migrate/copy.rs` 的数据复制同样在每张表结束时回报一条 `-- copy {n} rows into {table}`。`classify` 对注释开头的文本返回 `Other`，审计版按摘要记录。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p dblens-service --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service
git commit -m "feat(policy): 导出经过策略，所有写路径回报执行结果"
```

### Task 3: 凭据和连接配置的来源

**Files:**
- Create: `crates/dblens-service/src/source.rs`
- Modify: `crates/dblens-service/src/service.rs:25-95,97-103,154-178`
- Modify: `crates/dblens-service/src/metadata.rs`（字段和 199、259 行）
- Modify: `compare.rs:420`、`design.rs:386`、`export/job.rs:227`、`import/job.rs:295`、`migrate/mod.rs:495-496`、`privilege.rs:343`、`sqlfile.rs:161`
- Test: `crates/dblens-service/src/service.rs`

**Interfaces:**
- Produces:
  - `#[async_trait] trait CredentialSource: Send + Sync { async fn password(&self, profile_id: &str) -> Result<Option<String>>; }`
  - `trait ProfileSource: Send + Sync { fn list(&self) -> Result<Vec<Profile>>; fn get(&self, id: &str) -> Result<Profile>; }`
  - `Service::with_sources(store, secrets, registry, policy, history, credentials, profiles) -> Service`
  - `Service::password(&self, profile_id: &str) -> Result<String>`（缺失时返回空串，同现有 `unwrap_or_default`）

- [ ] **Step 1: 写失败的测试**

`service.rs` 测试模块：

```rust
    struct FixedCredential;

    #[async_trait::async_trait]
    impl CredentialSource for FixedCredential {
        async fn password(&self, _: &str) -> Result<Option<String>> {
            Ok(Some("pw".into()))
        }
    }

    #[tokio::test]
    async fn sessions_use_the_configured_credential_source() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
        let log: Log = Arc::default();
        let registry = Arc::new(DriverRegistry::with_drivers(
            store.clone(),
            vec![Box::new(FakeDriver { log })],
        ));
        let s = Service::with_sources(
            store.clone(),
            secrets,
            registry,
            Arc::new(DefaultPolicy),
            HistoryLog::off(),
            Arc::new(FixedCredential),
            store.clone(),
        );
        // 密码库没有初始化，也没有存密码；FakeDriver 只接受 pw
        let p = s.store.create_profile(input("pg")).unwrap();
        s.open_session(&p.id).await.unwrap();
    }
```

- [ ] **Step 2: 确认失败**

Run: `cargo test -p dblens-service --lib sessions_use_the_configured`
Expected: 编译失败，找不到 `CredentialSource`。

- [ ] **Step 3: 实现 `source.rs`**

```rust
//! 凭据和连接配置从哪里来（审计版设计 3.1 节）。单机版取本机的钥匙串或密码库、本地库；审计版由服务端托管。

use async_trait::async_trait;
use dblens_core::Result;

use crate::profile::Profile;
use crate::secret::Secrets;
use crate::store::Store;

#[async_trait]
pub trait CredentialSource: Send + Sync {
    async fn password(&self, profile_id: &str) -> Result<Option<String>>;
}

#[async_trait]
impl CredentialSource for Secrets {
    async fn password(&self, profile_id: &str) -> Result<Option<String>> {
        self.get(profile_id)
    }
}

/// 只读：连接的增删改只有单机版的路由用。
pub trait ProfileSource: Send + Sync {
    fn list(&self) -> Result<Vec<Profile>>;
    fn get(&self, id: &str) -> Result<Profile>;
}

impl ProfileSource for Store {
    fn list(&self) -> Result<Vec<Profile>> {
        self.list_profiles()
    }
    fn get(&self, id: &str) -> Result<Profile> {
        self.profile(id)
    }
}
```

`service.rs`：
- `Service` 增加字段 `credentials: Arc<dyn CredentialSource>`、`profiles: Arc<dyn ProfileSource>`；
- `with_history` 改为调用 `with_sources`，传 `secrets.clone()`、`store.clone()`；
- `MetadataService` 的 `secrets` 字段换成 `Arc<dyn CredentialSource>`；
- `profiles()`、`profile()` 走 `self.profiles`，155、162、174 行也走它（132、141 行是写操作的存在性检查，留在 `store`）；
- 新增：

```rust
    /// 连接的密码：没有保存时为空。所有建连路径都从这里取。
    pub(crate) async fn password(&self, profile_id: &str) -> Result<String> {
        Ok(self.credentials.password(profile_id).await?.unwrap_or_default())
    }
```

把任务 Files 里列出的每处 `self.secrets.get(&x)?.unwrap_or_default()` 换成 `self.password(&x).await?`。`metadata.rs` 两处换成 `self.credentials.password(&profile.id).await?.unwrap_or_default()`。

`store.profile` 留在 `Store` 内部的调用和 `service.rs` 的写路径不动。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p dblens-service --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service
git commit -m "feat(service): 凭据和连接配置改由可替换的来源提供"
```

### Task 4: 会话、任务、导出、上传的归属

**Files:**
- Modify: `crates/dblens-service/src/session.rs:242-270,326-345`
- Modify: `crates/dblens-service/src/task.rs:134,249,353-370,391-437`
- Modify: `crates/dblens-service/src/export/job.rs:83,146-190`
- Modify: `crates/dblens-service/src/sqlfile.rs` 等后台任务内调用 `tasks` 取上传的位置（只读核对）
- Test: 同文件测试模块

**Interfaces:**
- Consumes: `Operator::current()`、`Operator::scope`
- Produces: 其他操作者访问时返回和“不存在”相同的 `Error::NotFound`，不暴露存在性；`SessionManager::list` 只返回当前操作者的会话

- [ ] **Step 1: 写失败的测试**

`session.rs` 测试模块：

```rust
    #[tokio::test]
    async fn sessions_belong_to_their_operator() {
        let (m, _) = manager();
        let (id, _) = open(&m, Environment::Dev);
        let other = Operator { id: "other".into(), name: String::new() };
        let r = Operator::scope(other.clone(), async { m.info(id) }).await;
        assert!(matches!(r, Err(Error::NotFound { .. })));
        assert!(Operator::scope(other, async { m.list() }).await.unwrap().is_empty());
        assert!(m.info(id).is_ok(), "所有者不受影响");
    }
```

`task.rs` 测试模块：

```rust
    #[tokio::test]
    async fn tasks_and_uploads_belong_to_their_operator() {
        let m = TaskManager::new(tmp("r"), tmp("u"));
        let id = {
            let t = m.create(TaskKind::SqlFile).unwrap();
            t.id.clone()
        };
        let other = Operator { id: "other".into(), name: String::new() };
        assert!(matches!(
            Operator::scope(other.clone(), async { m.status(&id) }).await,
            Err(Error::NotFound { .. })
        ));
        let (up, path) = m.begin_upload().unwrap();
        std::fs::write(&path, "x").unwrap();
        m.finish_upload(&up, "a.sql", 1).unwrap();
        let src = FileSource::Upload { upload: up };
        assert!(Operator::scope(other, async { m.resolve(&src, false, false) }).await.is_err());
        assert!(m.resolve(&src, false, false).is_ok());
    }
```

`tmp` 用该模块现有的临时目录写法替换。`export/job.rs` 加同样形式的测试：用另一个操作者调用 `status`、`take_file`，都得到 `NotFound`。

- [ ] **Step 2: 确认失败**

Run: `cargo test -p dblens-service --lib belong_to_their_operator`
Expected: FAIL。

- [ ] **Step 3: 实现**

`session.rs`：`Handle` 增加 `owner: String`，`open` 时取 `Operator::current().id`。

```rust
    fn handle<T>(&self, id: SessionId, f: impl FnOnce(&Handle) -> T) -> Result<T> {
        let sessions = self
            .sessions
            .lock()
            .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "sessions")))?;
        let me = Operator::current().id;
        sessions
            .get(&id)
            .filter(|h| h.owner == me)
            .map(f)
            .ok_or_else(|| Error::not_found(msg!("noun.session", id = id)))
    }
```

`list` 加同样的过滤。`SessionInfo` 不加字段，前端协议不变。

`task.rs`：`Entry`、`Upload` 增加 `owner: String`，创建时取 `Operator::current().id`。`get` 和 `resolve` 的上传分支校验：

```rust
    fn get(&self, id: &str) -> Result<Arc<Entry>> {
        let me = Operator::current().id;
        self.tasks
            .lock()
            .map_err(|_| lock_error())?
            .get(id)
            .filter(|e| e.owner == me)
            .cloned()
            .ok_or_else(|| not_found(id))
    }
```

后台任务持有 `TaskHandle`，不经过 `get`，不受影响。`resolve` 在 `start_*` 里调用时操作者已设定；若某路径在 `tokio::spawn` 内调用 `resolve`（`document/job.rs:273` 要核对），在 spawn 前取出 `Operator::current()`，用 `Operator::scope` 包住任务体。

`export/job.rs`：`Job` 增加 `owner`，`status`、`cancel_or_remove`、`take_file` 取到后校验，不符返回 `not_found(id)`。`running()` 仍统计全部，限额是全局的。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p dblens-service --lib`
Expected: 全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service
git commit -m "feat(service): 会话、任务、导出和上传按操作者隔离"
```

### Task 5: API 路由可组合，能力查询，WebSocket 传递操作者

**Files:**
- Modify: `crates/dblens-api/src/lib.rs:56-177`
- Modify: `crates/dblens-api/src/ws.rs:109-119`（及文件内全部 `tokio::spawn`）
- Modify: `crates/dblens-api/src/routes.rs`
- Test: `crates/dblens-api/tests/api.rs`

**Interfaces:**
- Consumes: `Operator`
- Produces:
  - `dblens_api::api_routes() -> Router<AppState>`：不带鉴权和语言层
  - `pub use lang::negotiate`
  - `GET /api/capabilities` → `{"edition":"standalone","features":[]}`，只在 `router()` 里挂载，审计版自己实现
  - `dblens_service::Operator` 从 service 导出（API 层和私有仓库都用）

- [ ] **Step 1: 写失败的测试**

`crates/dblens-api/tests/api.rs`。现有辅助函数是 `app() -> (Router, Arc<Service>)`（第 20 行），带令牌的请求写法照该文件现有用例：

```rust
#[tokio::test]
async fn capabilities_report_standalone() {
    let (app, _) = app();
    let resp = app.oneshot(authed_get("/api/capabilities")).await.unwrap();
    assert_eq!(body_json(resp).await["edition"], "standalone");
}

#[tokio::test]
async fn routes_compose_under_a_custom_auth_layer() {
    use axum::{middleware, Router};
    let (_, service) = app();
    let config = ApiConfig { token: "unused".into(), allowed_origins: vec![], local_files: false };
    let state = AppState { service, config: Arc::new(config) };
    async fn as_user(req: axum::extract::Request, next: middleware::Next) -> axum::response::Response {
        let op = Operator { id: "u1".into(), name: "张三".into() };
        Operator::scope(op, next.run(req)).await
    }
    let app: Router = Router::new()
        .nest("/api", api_routes().layer(middleware::from_fn(as_user)).with_state(state));
    // 不带令牌也能访问，鉴权完全由外层决定
    let req = axum::http::Request::get("/api/sessions").body(axum::body::Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), 200);
}
```

`authed_get`、`body_json` 若文件里名字不同，用文件现有的等价写法，不要新增第二套辅助函数。

`crates/dblens-api/tests/` 下再加一个 WebSocket 用例：在 `Operator::scope` 外层中间件下开会话、经 WS 执行 `ROWS 1`，断言收到 `done`。写法复用 `tests/api.rs` 现有的 WS 测试。这个用例防止 `tokio::spawn` 丢失操作者导致 `NotFound`。

- [ ] **Step 2: 确认失败**

Run: `cargo test -p dblens-api --test api capabilities`
Expected: 编译失败。

- [ ] **Step 3: 实现**

`lib.rs` 把 `router` 拆成：

```rust
/// 全部业务接口，不带鉴权和语言层，由调用方决定怎么包。
pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/drivers", get(routes::drivers))
        // …原有 router() 里的全部 .route，顺序不变
        .route("/sessions/{id}/ws", get(ws::upgrade))
}

/// 单机版：带令牌鉴权。
pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
    let state = AppState { service, config: Arc::new(config) };
    let api = api_routes()
        .route("/capabilities", get(routes::capabilities))
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        // 最外层：令牌错误等也按请求语言输出
        .layer(middleware::from_fn(lang::negotiate))
        .with_state(state);
    Router::new().nest("/api", api)
}
```

`pub use lang::negotiate;`。

`routes.rs`：

```rust
/// 版本和能力：前端据此显示功能。审计版有自己的实现。
pub async fn capabilities() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "edition": "standalone", "features": [] }))
}
```

`ws.rs:118`：

```rust
    let op = Operator::current();
    Ok(ws.on_upgrade(move |socket| Operator::scope(op, scope_lang(lang, run(socket, service, id)))))
```

`ws.rs` 里每处 `tokio::spawn(pump(...))` 等后台任务，spawn 前取 `Operator::current()`，用 `Operator::scope` 包住，语言也按文件现有写法保持。

- [ ] **Step 4: 跑测试**

Run: `cargo test -p dblens-api`
Expected: 全部 PASS。

Run: `cargo build -p dblens-server -p dblens-desktop`
Expected: 成功。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-api crates/dblens-service/src/lib.rs
git commit -m "feat(api): 路由可组合，增加能力查询，WebSocket 传递操作者"
```

### Task 6: 架构测试锁住入口

**Files:**
- Create: `crates/dblens-service/tests/architecture.rs`

**Interfaces:**
- Consumes: Task 1–3 的收口结果

- [ ] **Step 1: 写测试**

```rust
//! 执行入口的约束（审计版设计 3.1 节）：新增的代码绕不过策略和凭据来源。
//! 扫描 src 下非测试部分的源码。

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// 文件中 `#[cfg(test)]` 之前的部分。
fn production(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    text.split("#[cfg(test)]").next().unwrap().to_string()
}

fn users_of(needle: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut found: Vec<String> = files
        .iter()
        .filter(|f| production(f).contains(needle))
        .map(|f| f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/"))
        .collect();
    found.sort();
    found
}

#[test]
fn only_ask_calls_before_execute() {
    assert_eq!(users_of(".before_execute("), ["policy.rs"]);
}

#[test]
fn passwords_come_from_the_credential_source() {
    assert_eq!(users_of(".secrets.get("), Vec::<String>::new());
}

#[test]
fn connections_are_opened_in_known_places() {
    // 新增建连路径时要先确认它经过策略，再加进这张表
    assert_eq!(
        users_of(".registry.connect("),
        [
            "design.rs",
            "export/job.rs",
            "import/job.rs",
            "metadata.rs",
            "migrate/run.rs",
            "privilege.rs",
            "service.rs",
            "sqlfile.rs",
        ]
    );
}
```

- [ ] **Step 2: 跑测试**

Run: `cargo test -p dblens-service --test architecture`
Expected: PASS。若失败，说明前面的任务有遗漏，回去修代码，不要放宽断言；`.registry.connect(` 列表与实际不符时，逐个确认该路径经过策略后再更新列表。

- [ ] **Step 3: 提交**

```bash
git add crates/dblens-service/tests/architecture.rs
git commit -m "test(service): 架构测试锁住策略和凭据入口"
```

### Task 7: 前端包入口和能力开关

**Files:**
- Create: `web/src/shared/capabilities.ts`、`web/src/shared/capabilities.test.ts`、`web/src/index.ts`
- Modify: `web/src/shared/api.ts`（`createApi` 返回对象内）
- Modify: `web/package.json`

不要改 `web/src/features/lint/` 下的文件。

**Interfaces:**
- Produces:
  - `api.capabilities(): Promise<Capabilities>`
  - `type Capabilities = { edition: 'standalone' | 'audit'; features: string[] }`
  - `hasFeature(c: Capabilities, name: string): boolean`
  - `useCapabilities(api): Capabilities`（加载前返回 `STANDALONE`）
  - 包入口导出 `App`、`createApi`、`useCapabilities`、`hasFeature`

- [ ] **Step 1: 写失败的测试**

`capabilities.test.ts`：

```ts
import { describe, expect, it } from 'vitest'
import { STANDALONE, hasFeature } from './capabilities'

describe('capabilities', () => {
  it('单机版没有审计版功能', () => {
    expect(STANDALONE.edition).toBe('standalone')
    expect(hasFeature(STANDALONE, 'tickets')).toBe(false)
  })
  it('按名称判断功能', () => {
    expect(hasFeature({ edition: 'audit', features: ['tickets'] }, 'tickets')).toBe(true)
  })
})
```

- [ ] **Step 2: 确认失败**

Run: `pnpm --dir web vitest run src/shared/capabilities.test.ts`
Expected: FAIL，模块不存在。

- [ ] **Step 3: 实现**

`capabilities.ts`：

```ts
import { useQuery } from '@tanstack/react-query'

export type Capabilities = { edition: 'standalone' | 'audit'; features: string[] }

export const STANDALONE: Capabilities = { edition: 'standalone', features: [] }

export function hasFeature(c: Capabilities, name: string): boolean {
  return c.features.includes(name)
}

/** 版本和能力。加载完成前按单机版显示。 */
export function useCapabilities(api: { capabilities(): Promise<Capabilities> }): Capabilities {
  const { data } = useQuery({ queryKey: ['capabilities'], queryFn: () => api.capabilities(), staleTime: Infinity })
  return data ?? STANDALONE
}
```

`api.ts` 的 `createApi` 返回对象增加（用文件内现有的请求封装）：

```ts
    capabilities: () => request<Capabilities>('GET', '/capabilities'),
```

`index.ts`：

```ts
export { default as App } from './app/App'
export { createApi } from './shared/api'
export { STANDALONE, hasFeature, useCapabilities } from './shared/capabilities'
export type { Capabilities } from './shared/capabilities'
```

`package.json` 增加 `"exports": { ".": "./src/index.ts" }`，`private` 保持 `true`。私有仓库以 workspace 源码方式引用，不发布到 npm。

- [ ] **Step 4: 跑测试**

Run: `pnpm --dir web test`
Expected: 全部 PASS。

Run: `pnpm --dir web typecheck`
Expected: 无错误。

- [ ] **Step 5: 提交**

```bash
git add web/src/shared/capabilities.ts web/src/shared/capabilities.test.ts web/src/index.ts web/src/shared/api.ts web/package.json
git commit -m "feat(web): 导出包入口，增加版本和能力开关"
```

### Task 8: 全量回归和说明

**Files:**
- Create: `docs/notes/g0-gate.md`
- Modify: `docs/superpowers/specs/2026-10-22-dblens-design.md`（第 11 节进度）

- [ ] **Step 1: 全量离线测试**

Run: `cargo test --workspace`
Expected: 全部 PASS（`#[ignore]` 的实连测试除外）。

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: 无警告。

- [ ] **Step 2: 实连回归**

按 `~/.dblens-test/test-env.ps1` 设好环境变量（只在环境变量里放密码），用硬超时跑 `crates/dblens-service/tests/live.rs`、`privilege_live.rs`、`migrate_live.rs`、`crates/dblens-api/tests/live.rs`，串行：

Run: `cargo test -p dblens-service --test live -- --ignored --test-threads=1`
Expected: PASS。其余三个文件同样运行。

- [ ] **Step 3: 界面回归**

启动 `dblens-server`，用真实鼠标走一遍：编辑器执行、生产连接确认弹窗、数据编辑保存、表设计器执行、导出下载、SQL 文件导入、账号权限修改。确认行为和改造前一致，并截图。

- [ ] **Step 4: 写说明**

`docs/notes/g0-gate.md` 记录：改了什么、四个扩展点（`Policy`、`CredentialSource`、`ProfileSource`、`api_routes`）的用法、`RequireTicket` 当前按拒绝处理、迁移读源库留给 G2、规格细化的五条。

总设计第 11 节进度追加：

```markdown
- 2026-10-31 用户决定分单机版和审计版，审计版走执行网关，取代原“团队版”（设计 `docs/superpowers/specs/2026-10-31-audit-edition-design.md`）。
- G0 已完成（详细计划 `docs/superpowers/plans/2026-10-31-g0-gate-extension-points.md`，说明 `docs/notes/g0-gate.md`）。
```

- [ ] **Step 5: 提交**

```bash
git add docs/notes/g0-gate.md docs/superpowers/specs/2026-10-22-dblens-design.md
git commit -m "docs: G0 实现说明"
```
