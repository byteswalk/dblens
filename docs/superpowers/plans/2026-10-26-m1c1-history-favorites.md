# M1c1 历史与收藏 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 记录每条执行过的语句并在历史侧栏中查找、复用；用多级文件夹收藏常用 SQL。

**Architecture:** 后端在会话执行每条语句后把历史交给后台线程批量写入本地 SQLite（迁移到版本 3），收藏同样存在本地库；`dblens-api` 提供 `/history`、`/favorites` 接口。前端新增历史、收藏两个侧栏和收藏对话框，通过"插入 SQL"通道和新开标签复用 SQL；"收藏光标所在的语句"由 M1b2 的 SQL 语言 worker 分句。

**Tech Stack:** Rust（rusqlite、标准库线程与同步通道）、axum；React 19 + TypeScript、antd 6（Tree、TreeSelect）、@tanstack/react-query 5、Vitest。

**Spec:** `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`

## Global Constraints

- 历史全部记录（含生产连接），默认保留最近 10000 条，范围 100–1000000；设置可关（`ui.history.enabled` 为 `off`）、可调（`ui.history.limit`）、可清空。
- 历史的写入不阻塞执行；写入失败只记日志。
- 收藏不绑定连接，记下保存时的驱动和变体；多级文件夹，不能把文件夹移进自己或自己的下级。
- 依赖只用正式发布版，跟随最新；新版发布满 7 天再升级；本里程碑不引入新依赖。
- 前端给用户看的文字都用词条（中英文），英文区分单复数；后端错误文字用 `msg!` 词条；日志不翻译。
- 代码注释用中文；文件 UTF-8、LF 换行；前端代码风格：无分号、单引号、两格缩进。
- cargo 命令加 `--config "source.mirror.registry='sparse+https://rsproxy.cn/index/'"`（clippy 写在 `clippy` 之后），并设 `CARGO_INCREMENTAL=0`；后端测试在 `DBLENS_LANG=en` 下运行。
- 前端工具用 PowerShell 执行（`npm`/`npx` 在本机不可用，用 `pnpm exec`）。
- 测试数据库的密码只在命令行中以内联环境变量传入，不写进仓库、报告或提交信息。

## 执行说明

任务 2–7 的代码都已在临时副本中逐任务实现并通过检查（含 5 个实库和浏览器实测），执行时按顺序重放。新文件给出全文，修改的文件给出 diff（基于上一个任务完成后的状态，可直接 `git apply`）。`Cargo.lock`、`web/pnpm-lock.yaml` 本里程碑不变。

## 文件结构

```text
crates/dblens-service/src/{history.rs, favorite.rs}   历史、收藏的存储（新增）
crates/dblens-service/src/{store.rs, session.rs, service.rs, profile.rs, lib.rs}   迁移、写历史、组装
crates/dblens-core/locales/app.json                   收藏的错误文字
crates/dblens-api/src/{routes.rs, lib.rs}; tests/library.rs   接口与测试
docs/notes/m0-4b-api.md                               接口说明
web/src/shared/api.ts                                 接口客户端
web/src/features/history/                             历史侧栏（新增）
web/src/features/favorites/                           收藏侧栏、对话框（新增）
web/src/features/editor/{commands.ts, actions.tsx, QueryView.tsx, SqlEditor.tsx, Toolbar.tsx}   插入、收藏命令
web/src/features/sql/                                 光标所在的语句
web/src/app/{Shell.tsx, SettingsDialog.tsx, menus.ts, shell.css}   侧栏、设置、菜单、样式
docs/notes/m1c1-history-favorites.md                  说明文档（新增）
```

---

### Task 1: 依赖复查

按依赖原则（M1b2 设计文档 3.1 节）复查前端和后端的全部直接依赖。本里程碑不引入新依赖。

**Files:** 视复查结果而定（`web/package.json`、`Cargo.toml` 及锁文件）。

- [ ] **Step 1: 运行复查脚本**

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; $env:NPM_REGISTRY = 'https://registry.npmmirror.com'; node scripts/check-deps.mjs
```

Expected：2026-10-26 写计划时为"共 71 个直接依赖，0 个可升级"（antd 6.6.8、i18next 26.5.11、react-i18next 17.1.9、react-resizable-panels 4.15.0、uuid 1.29.0 均未满 7 天）。

- [ ] **Step 2: 升级（有可升级项时）**

输出中 `->` 的都升级：npm 用 `pnpm add 包@版本`（在 `web` 目录），Cargo 改工作区 `Cargo.toml` 后执行 `cargo update -p 包`。然后跑全量检查：

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected：全部通过。结果记进任务 7 的说明文档（"依赖"一节）。

- [ ] **Step 3: 提交（有改动时）**

```bash
git add web/package.json web/pnpm-lock.yaml Cargo.toml Cargo.lock
git commit -m "chore: 依赖复查：升级到发布满 7 天的最新正式版" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: 历史与收藏的存储

本地库迁移到版本 3，新增 `history`、`favorites` 两张表。`history.rs` 负责历史的写入、清理、查询、删除和设置，以及后台写入线程 `HistoryLog`；`favorite.rs` 负责收藏的增删改和移动。新增的错误文字加入后端词条。

**Files:**
- Modify: `crates/dblens-core/locales/app.json`
- Create: `crates/dblens-service/src/favorite.rs`
- Create: `crates/dblens-service/src/history.rs`
- Modify: `crates/dblens-service/src/lib.rs`
- Modify: `crates/dblens-service/src/store.rs`

**Interfaces:**
- Produces（`dblens_service`）：
  - `NewHistory { at, profile_id, profile_name, driver, variant, schema, sql, elapsed_ms, rows, error }`、`HistoryItem { id, ..NewHistory }`（序列化时平铺）、`HistoryQuery { q, profile, failed, before, limit }`；
  - `Store::add_history(&[NewHistory], limit)`、`history(&HistoryQuery)`、`delete_history(id)`、`clear_history()`、`history_settings() -> (bool, u64)`；
  - `HISTORY_ENABLED_SETTING = "ui.history.enabled"`、`HISTORY_LIMIT_SETTING = "ui.history.limit"`、`DEFAULT_HISTORY_LIMIT = 10000`；
  - `HistoryLog::{off, inline(store), start(store)}`、`record(NewHistory)`；
  - `Favorite`、`FavoriteKind::{Folder, Item}`、`NewFavorite`、`FavoritePatch { name, sql, note, move_to（JSON 字段 move） }`、`FavoriteMove { parent_id, position }`；
  - `Store::favorites()`、`create_favorite(NewFavorite)`、`update_favorite(id, FavoritePatch)`、`delete_favorite(id)`；
  - `Store::lock()`（crate 内）。

- [ ] **Step 1: 修改 `crates/dblens-core/locales/app.json`**

```diff
--- a/crates/dblens-core/locales/app.json
+++ b/crates/dblens-core/locales/app.json
@@ -38,6 +38,11 @@
   "error.kind.not_found": {"zh-CN": "找不到%{what}", "en": "Not found: %{what}"},
   "error.kind.sql": {"zh-CN": "SQL 错误 [%{code}]：%{message}", "en": "SQL error [%{code}]: %{message}"},
   "error.kind.timeout": {"zh-CN": "执行超时", "en": "Execution timed out"},
+  "favorite.folder_has_no_sql": {"zh-CN": "文件夹不能有 SQL", "en": "A folder has no SQL"},
+  "favorite.move_into_self": {"zh-CN": "不能把文件夹移到它自己或它的子文件夹中", "en": "Cannot move a folder into itself or one of its subfolders"},
+  "favorite.name_required": {"zh-CN": "名称不能为空", "en": "The name is required"},
+  "favorite.parent_not_folder": {"zh-CN": "只能放在文件夹中", "en": "Favorites can only be placed in a folder"},
+  "favorite.sql_required": {"zh-CN": "收藏的 SQL 不能为空", "en": "The SQL of a favorite is required"},
   "internal.lock_poisoned": {"zh-CN": "内部锁已损坏（%{lock}）", "en": "Internal lock poisoned (%{lock})"},
   "internal.task_failed": {"zh-CN": "后台任务异常退出（%{task}）：%{error}", "en": "Background task failed (%{task}): %{error}"},
   "literal.bad_number": {"zh-CN": "不是有效的数字文本，无法生成字面量：%{value}", "en": "Not a valid number, cannot make a literal: %{value}"},
@@ -71,6 +76,8 @@
   "native.install.oracle_client": {"zh-CN": "请安装 Oracle Instant Client（Basic 或 Basic Light），然后在连接设置中指定其目录，或设置 ORACLE_HOME / PATH", "en": "Install Oracle Instant Client (Basic or Basic Light), then set its directory in the connection settings, or set ORACLE_HOME / PATH"},
   "native.not_found": {"zh-CN": "未找到%{lib}（%{file}）。%{install}。已查找：%{tried}", "en": "%{lib} (%{file}) not found. %{install}. Searched: %{tried}"},
   "native.oracle_client": {"zh-CN": "Oracle 客户端库", "en": "Oracle client library"},
+  "noun.favorite": {"zh-CN": "收藏 %{id}", "en": "favorite %{id}"},
+  "noun.history": {"zh-CN": "历史记录 %{id}", "en": "history entry %{id}"},
   "noun.profile": {"zh-CN": "连接配置 %{id}", "en": "connection profile %{id}"},
   "noun.session": {"zh-CN": "会话 %{id}", "en": "session %{id}"},
   "oracle.bad_chars": {"zh-CN": "Oracle %{what}含有不允许的字符：%{value}", "en": "The Oracle %{what} contains characters that are not allowed: %{value}"},
```

- [ ] **Step 2: 新建 `crates/dblens-service/src/favorite.rs`**

```rust
//! 收藏（M1c1 设计文档第 3 节）：多级文件夹和收藏的 SQL 放在同一张表里，`parent_id` 指向上级文件夹。
//! 收藏不绑定连接，只记下保存时的数据库类型供界面显示图标。

use std::time::{SystemTime, UNIX_EPOCH};

use dblens_core::{Error, Result, msg};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FavoriteKind {
    Folder,
    Item,
}

impl FavoriteKind {
    fn as_str(self) -> &'static str {
        match self {
            FavoriteKind::Folder => "folder",
            FavoriteKind::Item => "item",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Favorite {
    pub id: String,
    /// 上级文件夹；None 为根。
    pub parent_id: Option<String>,
    pub kind: FavoriteKind,
    pub name: String,
    /// 收藏的 SQL；文件夹为 None。
    pub sql: Option<String>,
    pub note: Option<String>,
    /// 保存时的驱动和变体（图标用）。
    pub driver: Option<String>,
    pub variant: Option<String>,
    /// 同一文件夹内的次序，从 0 开始。
    pub position: i64,
    /// Unix 毫秒。
    pub created_at: i64,
    pub updated_at: i64,
}

/// 新建文件夹或收藏。放在上级文件夹的末尾。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NewFavorite {
    #[serde(default)]
    pub parent_id: Option<String>,
    pub kind: FavoriteKind,
    pub name: String,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
}

/// 移动到某个文件夹（None 为根）的第 `position` 个位置。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FavoriteMove {
    pub parent_id: Option<String>,
    pub position: i64,
}

/// 修改：给出的字段才改。`note` 为空字符串时清除说明。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct FavoritePatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, rename = "move")]
    pub move_to: Option<FavoriteMove>,
}

const COLUMNS: &str =
    "id, parent_id, kind, name, sql, note, driver, variant, position, created_at, updated_at";

fn from_row(r: &Row) -> rusqlite::Result<Favorite> {
    let kind: String = r.get(2)?;
    Ok(Favorite {
        id: r.get(0)?,
        parent_id: r.get(1)?,
        kind: if kind == "folder" { FavoriteKind::Folder } else { FavoriteKind::Item },
        name: r.get(3)?,
        sql: r.get(4)?,
        note: r.get(5)?,
        driver: r.get(6)?,
        variant: r.get(7)?,
        position: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

fn not_found(id: &str) -> Error {
    Error::not_found(msg!("noun.favorite", id = id))
}

fn get(c: &Connection, id: &str) -> rusqlite::Result<Option<Favorite>> {
    c.query_row(&format!("SELECT {COLUMNS} FROM favorites WHERE id = ?1"), params![id], from_row).optional()
}

/// 同一文件夹中的其他项（按次序），不含 `except`。
fn siblings(c: &Connection, parent: Option<&str>, except: &str) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT id FROM favorites WHERE parent_id IS ?1 AND id <> ?2 ORDER BY position, created_at, id",
    )?;
    st.query_map(params![parent, except], |r| r.get(0))?.collect()
}

fn renumber(c: &Connection, ids: &[String]) -> rusqlite::Result<()> {
    let mut st = c.prepare("UPDATE favorites SET position = ?2 WHERE id = ?1 AND position <> ?2")?;
    for (i, id) in ids.iter().enumerate() {
        st.execute(params![id, i as i64])?;
    }
    Ok(())
}

/// `parent` 必须是存在的文件夹。
fn check_parent(c: &Connection, parent: Option<&str>) -> Result<()> {
    let Some(p) = parent else { return Ok(()) };
    match get(c, p).map_err(crate::store::db_err)? {
        Some(f) if f.kind == FavoriteKind::Folder => Ok(()),
        Some(_) => Err(Error::invalid(msg!("favorite.parent_not_folder"))),
        None => Err(not_found(p)),
    }
}

fn trimmed_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::invalid(msg!("favorite.name_required")));
    }
    Ok(name.to_string())
}

impl Store {
    /// 全部文件夹和收藏，按上级和次序排列。
    pub fn favorites(&self) -> Result<Vec<Favorite>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {COLUMNS} FROM favorites ORDER BY parent_id, position, created_at, id"
            ))?;
            st.query_map([], from_row)?.collect()
        })
    }

    pub fn create_favorite(&self, new: NewFavorite) -> Result<Favorite> {
        let name = trimmed_name(&new.name)?;
        let sql = match new.kind {
            FavoriteKind::Item => match new.sql.filter(|s| !s.trim().is_empty()) {
                Some(s) => Some(s),
                None => return Err(Error::invalid(msg!("favorite.sql_required"))),
            },
            FavoriteKind::Folder => None,
        };
        let id = uuid::Uuid::new_v4().to_string();
        let ts = now_ms();
        let mut conn = self.lock()?;
        check_parent(&conn, new.parent_id.as_deref())?;
        let tx = conn.transaction().map_err(crate::store::db_err)?;
        let position =
            siblings(&tx, new.parent_id.as_deref(), &id).map_err(crate::store::db_err)?.len() as i64;
        tx.execute(
            &format!(
                "INSERT INTO favorites ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)"
            ),
            params![
                id,
                new.parent_id,
                new.kind.as_str(),
                name,
                sql,
                new.note.filter(|n| !n.trim().is_empty()),
                new.driver,
                new.variant,
                position,
                ts
            ],
        )
        .map_err(crate::store::db_err)?;
        let f = get(&tx, &id).map_err(crate::store::db_err)?.ok_or_else(|| not_found(&id))?;
        tx.commit().map_err(crate::store::db_err)?;
        Ok(f)
    }

    pub fn update_favorite(&self, id: &str, patch: FavoritePatch) -> Result<Favorite> {
        let mut conn = self.lock()?;
        let cur = get(&conn, id).map_err(crate::store::db_err)?.ok_or_else(|| not_found(id))?;
        let name = match &patch.name {
            Some(n) => trimmed_name(n)?,
            None => cur.name.clone(),
        };
        let sql = match (cur.kind, patch.sql) {
            (FavoriteKind::Item, Some(s)) if s.trim().is_empty() => {
                return Err(Error::invalid(msg!("favorite.sql_required")));
            }
            (FavoriteKind::Item, Some(s)) => Some(s),
            (FavoriteKind::Folder, Some(_)) => {
                return Err(Error::invalid(msg!("favorite.folder_has_no_sql")));
            }
            (_, None) => cur.sql.clone(),
        };
        let note = match patch.note {
            Some(n) if n.trim().is_empty() => None,
            Some(n) => Some(n),
            None => cur.note.clone(),
        };
        if let Some(m) = &patch.move_to {
            check_parent(&conn, m.parent_id.as_deref())?;
            // 不能移进自己或自己的下级
            let mut p = m.parent_id.clone();
            while let Some(pid) = p {
                if pid == id {
                    return Err(Error::invalid(msg!("favorite.move_into_self")));
                }
                p = get(&conn, &pid).map_err(crate::store::db_err)?.and_then(|f| f.parent_id);
            }
        }
        let tx = conn.transaction().map_err(crate::store::db_err)?;
        tx.execute(
            "UPDATE favorites SET name = ?2, sql = ?3, note = ?4, updated_at = ?5 WHERE id = ?1",
            params![id, name, sql, note, now_ms()],
        )
        .map_err(crate::store::db_err)?;
        if let Some(m) = patch.move_to {
            let mut ids = siblings(&tx, m.parent_id.as_deref(), id).map_err(crate::store::db_err)?;
            let at = m.position.clamp(0, ids.len() as i64) as usize;
            ids.insert(at, id.to_string());
            tx.execute("UPDATE favorites SET parent_id = ?2 WHERE id = ?1", params![id, m.parent_id])
                .map_err(crate::store::db_err)?;
            renumber(&tx, &ids).map_err(crate::store::db_err)?;
            // 原文件夹中的次序重新编号，不留空位
            if cur.parent_id != m.parent_id {
                let old = siblings(&tx, cur.parent_id.as_deref(), id).map_err(crate::store::db_err)?;
                renumber(&tx, &old).map_err(crate::store::db_err)?;
            }
        }
        let f = get(&tx, id).map_err(crate::store::db_err)?.ok_or_else(|| not_found(id))?;
        tx.commit().map_err(crate::store::db_err)?;
        Ok(f)
    }

    /// 删除；文件夹连同其下的内容一起删除。
    pub fn delete_favorite(&self, id: &str) -> Result<()> {
        let mut conn = self.lock()?;
        let cur = get(&conn, id).map_err(crate::store::db_err)?.ok_or_else(|| not_found(id))?;
        let tx = conn.transaction().map_err(crate::store::db_err)?;
        tx.execute("DELETE FROM favorites WHERE id = ?1", params![id]).map_err(crate::store::db_err)?;
        let rest = siblings(&tx, cur.parent_id.as_deref(), id).map_err(crate::store::db_err)?;
        renumber(&tx, &rest).map_err(crate::store::db_err)?;
        tx.commit().map_err(crate::store::db_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(parent: Option<&str>, name: &str) -> NewFavorite {
        NewFavorite {
            parent_id: parent.map(str::to_string),
            kind: FavoriteKind::Folder,
            name: name.into(),
            sql: None,
            note: None,
            driver: None,
            variant: None,
        }
    }

    fn item(parent: Option<&str>, name: &str) -> NewFavorite {
        NewFavorite {
            kind: FavoriteKind::Item,
            sql: Some(format!("SELECT '{name}'")),
            driver: Some("oracle".into()),
            ..folder(parent, name)
        }
    }

    /// 某个文件夹中的名称，按次序。
    fn names(s: &Store, parent: Option<&str>) -> Vec<String> {
        s.favorites()
            .unwrap()
            .into_iter()
            .filter(|f| f.parent_id.as_deref() == parent)
            .map(|f| f.name)
            .collect()
    }

    #[test]
    fn create_appends_and_validates() {
        let s = Store::open_in_memory().unwrap();
        let f = s.create_favorite(folder(None, "  日报 ")).unwrap();
        assert_eq!(
            (f.name.as_str(), f.kind, f.position, f.sql.clone()),
            ("日报", FavoriteKind::Folder, 0, None)
        );
        let a = s.create_favorite(item(Some(&f.id), "a")).unwrap();
        let b =
            s.create_favorite(NewFavorite { note: Some("说明".into()), ..item(Some(&f.id), "b") }).unwrap();
        assert_eq!((a.position, b.position), (0, 1));
        assert_eq!(
            (b.sql.as_deref(), b.note.as_deref(), b.driver.as_deref()),
            (Some("SELECT 'b'"), Some("说明"), Some("oracle"))
        );
        let invalid = |r: Result<Favorite>| matches!(r, Err(Error::Invalid { .. }));
        assert!(invalid(s.create_favorite(folder(None, " "))));
        assert!(invalid(s.create_favorite(NewFavorite { sql: Some(" ".into()), ..item(None, "x") })));
        assert!(invalid(s.create_favorite(item(Some(&a.id), "x"))), "上级必须是文件夹");
        assert!(matches!(s.create_favorite(item(Some("nope"), "x")), Err(Error::NotFound { .. })));
    }

    #[test]
    fn update_fields() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_favorite(NewFavorite { note: Some("旧".into()), ..item(None, "a") }).unwrap();
        let patch =
            FavoritePatch { name: Some("新名".into()), sql: Some("SELECT 2".into()), ..Default::default() };
        let u = s.update_favorite(&a.id, patch).unwrap();
        assert_eq!(
            (u.name.as_str(), u.sql.as_deref(), u.note.as_deref()),
            ("新名", Some("SELECT 2"), Some("旧"))
        );
        let u = s
            .update_favorite(&a.id, FavoritePatch { note: Some(String::new()), ..Default::default() })
            .unwrap();
        assert_eq!(u.note, None, "空说明表示清除");
        let d = s.create_favorite(folder(None, "d")).unwrap();
        let r = s.update_favorite(&d.id, FavoritePatch { sql: Some("x".into()), ..Default::default() });
        assert!(matches!(r, Err(Error::Invalid { .. })), "文件夹没有 SQL");
        assert!(matches!(s.update_favorite("nope", FavoritePatch::default()), Err(Error::NotFound { .. })));
    }

    #[test]
    fn move_reorders_and_renumbers() {
        let s = Store::open_in_memory().unwrap();
        let ids: Vec<String> =
            ["a", "b", "c"].iter().map(|n| s.create_favorite(item(None, n)).unwrap().id).collect();
        let d = s.create_favorite(folder(None, "d")).unwrap();
        let mv = |id: &str, parent: Option<&str>, position: i64| {
            let move_to = Some(FavoriteMove { parent_id: parent.map(str::to_string), position });
            s.update_favorite(id, FavoritePatch { move_to, ..Default::default() }).unwrap()
        };
        mv(&ids[2], None, 0);
        assert_eq!(names(&s, None), ["c", "a", "b", "d"]);
        let moved = mv(&ids[0], Some(&d.id), 5);
        assert_eq!(
            (moved.parent_id.as_deref(), moved.position),
            (Some(d.id.as_str()), 0),
            "超出末尾时放在最后"
        );
        assert_eq!(names(&s, None), ["c", "b", "d"]);
        let positions: Vec<i64> =
            s.favorites().unwrap().iter().filter(|f| f.parent_id.is_none()).map(|f| f.position).collect();
        assert_eq!(positions, [0, 1, 2], "原文件夹重新编号");
        mv(&ids[0], None, 1);
        assert_eq!(names(&s, None), ["c", "a", "b", "d"]);
    }

    #[test]
    fn cannot_move_folder_into_itself_or_descendant() {
        let s = Store::open_in_memory().unwrap();
        let top = s.create_favorite(folder(None, "top")).unwrap();
        let sub = s.create_favorite(folder(Some(&top.id), "sub")).unwrap();
        for target in [&top.id, &sub.id] {
            let move_to = Some(FavoriteMove { parent_id: Some(target.clone()), position: 0 });
            let r = s.update_favorite(&top.id, FavoritePatch { move_to, ..Default::default() });
            assert!(matches!(r, Err(Error::Invalid { .. })), "{target}");
        }
    }

    #[test]
    fn delete_folder_removes_contents_and_renumbers() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_favorite(item(None, "a")).unwrap();
        let top = s.create_favorite(folder(None, "top")).unwrap();
        let sub = s.create_favorite(folder(Some(&top.id), "sub")).unwrap();
        s.create_favorite(item(Some(&sub.id), "x")).unwrap();
        s.create_favorite(item(None, "b")).unwrap();
        s.delete_favorite(&top.id).unwrap();
        let all = s.favorites().unwrap();
        assert_eq!(
            all.iter().map(|f| (f.name.as_str(), f.position)).collect::<Vec<_>>(),
            [("a", 0), ("b", 1)]
        );
        assert!(matches!(s.delete_favorite(&top.id), Err(Error::NotFound { .. })));
        s.delete_favorite(&a.id).unwrap();
        assert_eq!(s.favorites().unwrap()[0].position, 0);
    }
}
```

- [ ] **Step 3: 新建 `crates/dblens-service/src/history.rs`**

```rust
//! 执行历史（M1c1 设计文档第 3、4 节）：每条执行过的语句一条记录，保存在本地库，超出上限时删除最旧的。
//! 写入不阻塞执行：会话把记录交给 `HistoryLog`，由后台线程批量写库。

use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use dblens_core::{Error, Result, msg};
use rusqlite::{Row, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::store::Store;

/// 设置：值为 `off` 时不记录历史（已有记录保留）。
pub const HISTORY_ENABLED_SETTING: &str = "ui.history.enabled";
/// 设置：保留的条数。
pub const HISTORY_LIMIT_SETTING: &str = "ui.history.limit";
pub const DEFAULT_HISTORY_LIMIT: u64 = 10_000;
const MIN_HISTORY_LIMIT: u64 = 100;
const MAX_HISTORY_LIMIT: u64 = 1_000_000;

/// 一条要写入的历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewHistory {
    /// 执行开始时间，Unix 毫秒。
    pub at: i64,
    pub profile_id: String,
    /// 执行时的连接名（连接删除后仍能显示）。
    pub profile_name: String,
    pub driver: String,
    pub variant: Option<String>,
    /// 执行时的当前 schema。
    pub schema: Option<String>,
    /// 这一条语句的原文（不含结束符）。
    pub sql: String,
    pub elapsed_ms: Option<u64>,
    /// 查询取回的行数，或增删改影响的行数；不知道时为 None。
    pub rows: Option<u64>,
    /// 失败、被拒绝或被取消时的错误文字；成功时为 None。
    pub error: Option<String>,
}

/// 查询返回的一条历史。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryItem {
    pub id: i64,
    #[serde(flatten)]
    pub entry: NewHistory,
}

/// 历史查询条件：按时间倒序分页。
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct HistoryQuery {
    /// 在 SQL 中做不区分大小写的包含匹配。
    #[serde(default)]
    pub q: Option<String>,
    /// 只看这个连接的。
    #[serde(default)]
    pub profile: Option<String>,
    /// 只看有错误的（失败、被拒绝、被取消）。
    #[serde(default)]
    pub failed: bool,
    /// 上一页最后一条的 id，返回比它旧的。
    #[serde(default)]
    pub before: Option<i64>,
    /// 每页条数，默认 100，最大 500。
    #[serde(default)]
    pub limit: Option<u32>,
}

const COLUMNS: &str = "at, profile_id, profile_name, driver, variant, schema, sql, elapsed_ms, rows, error";

fn from_row(r: &Row) -> rusqlite::Result<HistoryItem> {
    Ok(HistoryItem {
        id: r.get(0)?,
        entry: NewHistory {
            at: r.get(1)?,
            profile_id: r.get(2)?,
            profile_name: r.get(3)?,
            driver: r.get(4)?,
            variant: r.get(5)?,
            schema: r.get(6)?,
            sql: r.get(7)?,
            elapsed_ms: r.get::<_, Option<i64>>(8)?.map(|v| v as u64),
            rows: r.get::<_, Option<i64>>(9)?.map(|v| v as u64),
            error: r.get(10)?,
        },
    })
}

impl Store {
    /// 写入一批历史，然后删除超出 `limit` 条的最旧记录。
    pub fn add_history(&self, entries: &[NewHistory], limit: u64) -> Result<()> {
        self.with(|c| {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare(&format!(
                    "INSERT INTO history ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
                ))?;
                for e in entries {
                    st.execute(params![
                        e.at,
                        e.profile_id,
                        e.profile_name,
                        e.driver,
                        e.variant,
                        e.schema,
                        e.sql,
                        e.elapsed_ms.map(|v| v as i64),
                        e.rows.map(|v| v as i64),
                        e.error
                    ])?;
                }
            }
            tx.execute(
                "DELETE FROM history WHERE id <= (SELECT id FROM history ORDER BY id DESC LIMIT 1 OFFSET ?1)",
                params![limit as i64],
            )?;
            tx.commit()
        })
    }

    pub fn history(&self, q: &HistoryQuery) -> Result<Vec<HistoryItem>> {
        let mut sql = format!("SELECT id, {COLUMNS} FROM history WHERE 1 = 1");
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(text) = q.q.as_deref().filter(|t| !t.is_empty()) {
            args.push(text.to_lowercase().into());
            sql += &format!(" AND instr(lower(sql), ?{}) > 0", args.len());
        }
        if let Some(p) = &q.profile {
            args.push(p.clone().into());
            sql += &format!(" AND profile_id = ?{}", args.len());
        }
        if q.failed {
            sql += " AND error IS NOT NULL";
        }
        if let Some(before) = q.before {
            args.push(before.into());
            sql += &format!(" AND id < ?{}", args.len());
        }
        let limit = q.limit.unwrap_or(100).clamp(1, 500);
        sql += &format!(" ORDER BY id DESC LIMIT {limit}");
        self.with(|c| {
            let mut st = c.prepare(&sql)?;
            st.query_map(params_from_iter(args), from_row)?.collect()
        })
    }

    pub fn delete_history(&self, id: i64) -> Result<()> {
        let n = self.with(|c| c.execute("DELETE FROM history WHERE id = ?1", params![id]))?;
        if n == 0 {
            return Err(Error::not_found(msg!("noun.history", id = id)));
        }
        Ok(())
    }

    pub fn clear_history(&self) -> Result<()> {
        self.with(|c| c.execute("DELETE FROM history", []))?;
        Ok(())
    }

    /// 历史设置：是否记录、保留条数（不合法时用默认值，超出范围时取边界）。
    pub fn history_settings(&self) -> Result<(bool, u64)> {
        let enabled = self.setting(HISTORY_ENABLED_SETTING)?.as_deref() != Some("off");
        let limit = self
            .setting(HISTORY_LIMIT_SETTING)?
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map_or(DEFAULT_HISTORY_LIMIT, |v| v.clamp(MIN_HISTORY_LIMIT, MAX_HISTORY_LIMIT));
        Ok((enabled, limit))
    }
}

/// 写历史的入口，由会话共用。克隆后指向同一个后台写入线程。
#[derive(Clone)]
pub struct HistoryLog {
    inner: Inner,
}

#[derive(Clone)]
enum Inner {
    /// 不记录（测试中不关心历史时用）。
    Off,
    /// 后台线程写入：有界队列，满了丢弃并记日志。
    Thread(SyncSender<NewHistory>),
    /// 直接在调用方写入（测试用，写完立即可查）。
    Inline(Arc<Store>),
}

/// 后台队列的容量。正常情况下写入很快，积压到这个数量说明本地库出了问题。
const QUEUE: usize = 1024;
/// 每批最多写入的条数。
const BATCH: usize = 200;

impl HistoryLog {
    pub fn off() -> Self {
        Self { inner: Inner::Off }
    }

    pub fn inline(store: Arc<Store>) -> Self {
        Self { inner: Inner::Inline(store) }
    }

    /// 启动后台写入线程。所有 `HistoryLog` 都被丢弃后线程退出。
    pub fn start(store: Arc<Store>) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        std::thread::Builder::new()
            .name("dblens-history".into())
            .spawn(move || writer(&store, &rx))
            .map(|_| Self { inner: Inner::Thread(tx) })
            .unwrap_or_else(|e| {
                tracing::warn!("无法启动历史写入线程，本次运行不记录历史：{e}");
                Self::off()
            })
    }

    /// 记录一条。不阻塞；失败只记日志。
    pub fn record(&self, entry: NewHistory) {
        match &self.inner {
            Inner::Off => {}
            Inner::Thread(tx) => match tx.try_send(entry) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => tracing::warn!("历史写入队列已满，丢弃一条历史"),
                Err(TrySendError::Disconnected(_)) => tracing::warn!("历史写入线程已退出，丢弃一条历史"),
            },
            Inner::Inline(store) => write_batch(store, &[entry]),
        }
    }
}

fn writer(store: &Store, rx: &Receiver<NewHistory>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while batch.len() < BATCH {
            match rx.try_recv() {
                Ok(e) => batch.push(e),
                Err(_) => break,
            }
        }
        write_batch(store, &batch);
    }
}

fn write_batch(store: &Store, batch: &[NewHistory]) {
    let result = store
        .history_settings()
        .and_then(|(enabled, limit)| if enabled { store.add_history(batch, limit) } else { Ok(()) });
    if let Err(e) = result {
        tracing::warn!("写入历史失败：{}", e.to_msg().current());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn entry(sql: &str, error: Option<&str>) -> NewHistory {
        NewHistory {
            at: 1_700_000_000_000,
            profile_id: "p1".into(),
            profile_name: "开发库".into(),
            driver: "postgres".into(),
            variant: Some("postgres".into()),
            schema: Some("public".into()),
            sql: sql.into(),
            elapsed_ms: Some(3),
            rows: Some(1),
            error: error.map(str::to_string),
        }
    }

    fn sqls(items: &[HistoryItem]) -> Vec<&str> {
        items.iter().map(|i| i.entry.sql.as_str()).collect()
    }

    #[test]
    fn newest_first_with_paging() {
        let s = Store::open_in_memory().unwrap();
        let batch: Vec<_> = (1..=5).map(|i| entry(&format!("SELECT {i}"), None)).collect();
        s.add_history(&batch, 100).unwrap();
        let page = s.history(&HistoryQuery { limit: Some(2), ..Default::default() }).unwrap();
        assert_eq!(sqls(&page), ["SELECT 5", "SELECT 4"]);
        let next = s
            .history(&HistoryQuery { before: Some(page[1].id), limit: Some(2), ..Default::default() })
            .unwrap();
        assert_eq!(sqls(&next), ["SELECT 3", "SELECT 2"]);
        assert_eq!(page[0].entry, entry("SELECT 5", None), "字段原样保存");
    }

    #[test]
    fn search_profile_and_failed_filters() {
        let s = Store::open_in_memory().unwrap();
        let mut other = entry("select * from Orders", None);
        other.profile_id = "p2".into();
        s.add_history(
            &[entry("SELECT * FROM emp", None), entry("UPDATE emp SET a = 1", Some("已取消")), other],
            100,
        )
        .unwrap();
        let q = |q: HistoryQuery| {
            sqls(&s.history(&q).unwrap()).into_iter().map(str::to_string).collect::<Vec<_>>()
        };
        assert_eq!(
            q(HistoryQuery { q: Some("ORDERS".into()), ..Default::default() }),
            ["select * from Orders"]
        );
        assert_eq!(q(HistoryQuery { q: Some("emp".into()), ..Default::default() }).len(), 2);
        assert_eq!(
            q(HistoryQuery { profile: Some("p2".into()), ..Default::default() }),
            ["select * from Orders"]
        );
        assert_eq!(q(HistoryQuery { failed: true, ..Default::default() }), ["UPDATE emp SET a = 1"]);
        assert_eq!(
            q(HistoryQuery { q: Some("%".into()), ..Default::default() }),
            Vec::<String>::new(),
            "% 不是通配符"
        );
    }

    #[test]
    fn keeps_only_the_newest_limit() {
        let s = Store::open_in_memory().unwrap();
        for i in 1..=7 {
            s.add_history(&[entry(&format!("S{i}"), None)], 3).unwrap();
        }
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["S7", "S6", "S5"]);
    }

    #[test]
    fn delete_and_clear() {
        let s = Store::open_in_memory().unwrap();
        s.add_history(&[entry("A", None), entry("B", None)], 100).unwrap();
        let items = s.history(&HistoryQuery::default()).unwrap();
        s.delete_history(items[0].id).unwrap();
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["A"]);
        assert!(matches!(s.delete_history(items[0].id), Err(Error::NotFound { .. })));
        s.clear_history().unwrap();
        assert!(s.history(&HistoryQuery::default()).unwrap().is_empty());
    }

    #[test]
    fn settings_have_defaults_and_bounds() {
        let s = Store::open_in_memory().unwrap();
        assert_eq!(s.history_settings().unwrap(), (true, DEFAULT_HISTORY_LIMIT));
        s.set_setting(HISTORY_LIMIT_SETTING, Some("5")).unwrap();
        s.set_setting(HISTORY_ENABLED_SETTING, Some("off")).unwrap();
        assert_eq!(s.history_settings().unwrap(), (false, MIN_HISTORY_LIMIT));
        s.set_setting(HISTORY_LIMIT_SETTING, Some("abc")).unwrap();
        assert_eq!(s.history_settings().unwrap().1, DEFAULT_HISTORY_LIMIT);
    }

    #[test]
    fn log_respects_enabled_setting() {
        let s = Arc::new(Store::open_in_memory().unwrap());
        let log = HistoryLog::inline(s.clone());
        log.record(entry("A", None));
        s.set_setting(HISTORY_ENABLED_SETTING, Some("off")).unwrap();
        log.record(entry("B", None));
        assert_eq!(sqls(&s.history(&HistoryQuery::default()).unwrap()), ["A"]);
    }

    #[test]
    fn background_thread_writes() {
        let s = Arc::new(Store::open_in_memory().unwrap());
        let log = HistoryLog::start(s.clone());
        for i in 0..50 {
            log.record(entry(&format!("S{i}"), None));
        }
        drop(log);
        for _ in 0..200 {
            if s.history(&HistoryQuery { limit: Some(500), ..Default::default() }).unwrap().len() == 50 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("后台线程没有写完");
    }
}
```

- [ ] **Step 4: 修改 `crates/dblens-service/src/lib.rs`**

```diff
--- a/crates/dblens-service/src/lib.rs
+++ b/crates/dblens-service/src/lib.rs
@@ -3,6 +3,8 @@
 /// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
 #[cfg(any(test, feature = "testing"))]
 pub mod fake;
+mod favorite;
+mod history;
 mod metadata;
 mod policy;
 mod profile;
@@ -12,6 +14,11 @@ mod service;
 mod session;
 mod store;
 
+pub use favorite::{Favorite, FavoriteKind, FavoriteMove, FavoritePatch, NewFavorite};
+pub use history::{
+    DEFAULT_HISTORY_LIMIT, HISTORY_ENABLED_SETTING, HISTORY_LIMIT_SETTING, HistoryItem, HistoryLog,
+    HistoryQuery, NewHistory,
+};
 pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
 pub use policy::{
     Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
```

- [ ] **Step 5: 修改 `crates/dblens-service/src/store.rs`**

```diff
--- a/crates/dblens-service/src/store.rs
+++ b/crates/dblens-service/src/store.rs
@@ -3,7 +3,7 @@
 
 use std::collections::BTreeMap;
 use std::path::{Path, PathBuf};
-use std::sync::Mutex;
+use std::sync::{Mutex, MutexGuard};
 
 use dblens_core::{Error, Result, msg};
 use rusqlite::{Connection, OptionalExtension, params};
@@ -43,6 +43,36 @@ const MIGRATIONS: &[&str] = &[
     );",
     // 版本 2：连接配置记住上次连接成功时识别出的数据库变体（界面据此显示图标）
     "ALTER TABLE profiles ADD COLUMN variant TEXT;",
+    // 版本 3：执行历史、收藏（M1c1）
+    "CREATE TABLE history (
+        id           INTEGER PRIMARY KEY,
+        at           INTEGER NOT NULL,
+        profile_id   TEXT NOT NULL,
+        profile_name TEXT NOT NULL,
+        driver       TEXT NOT NULL,
+        variant      TEXT,
+        schema       TEXT,
+        sql          TEXT NOT NULL,
+        elapsed_ms   INTEGER,
+        rows         INTEGER,
+        error        TEXT
+    );
+    CREATE INDEX history_at ON history(at);
+    CREATE INDEX history_profile ON history(profile_id, at);
+    CREATE TABLE favorites (
+        id         TEXT PRIMARY KEY,
+        parent_id  TEXT REFERENCES favorites(id) ON DELETE CASCADE,
+        kind       TEXT NOT NULL,
+        name       TEXT NOT NULL,
+        sql        TEXT,
+        note       TEXT,
+        driver     TEXT,
+        variant    TEXT,
+        position   INTEGER NOT NULL,
+        created_at INTEGER NOT NULL,
+        updated_at INTEGER NOT NULL
+    );
+    CREATE INDEX favorites_parent ON favorites(parent_id, position);",
 ];
 
 pub(crate) fn db_err(e: rusqlite::Error) -> Error {
@@ -97,11 +127,15 @@ impl Store {
 
     /// 在数据库连接上执行一段操作。操作都很快，直接持锁执行。
     pub(crate) fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T> {
-        let mut conn =
-            self.conn.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "store")))?;
+        let mut conn = self.lock()?;
         f(&mut conn).map_err(db_err)
     }
 
+    /// 取得数据库连接，供需要在一个事务中做校验和多步修改的操作使用。
+    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
+        self.conn.lock().map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "store")))
+    }
+
     pub fn setting(&self, key: &str) -> Result<Option<String>> {
         self.with(|c| {
             c.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0)).optional()
```

- [ ] **Step 6: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: fmt、clippy 无输出；全部测试通过（357 个，新增历史 7 个、收藏 5 个）。

- [ ] **Step 7: 提交**

```bash
git add crates/dblens-core/locales/app.json crates/dblens-service/src/favorite.rs crates/dblens-service/src/history.rs crates/dblens-service/src/lib.rs crates/dblens-service/src/store.rs
git commit -m "feat(service): 本地库迁移到版本 3：执行历史与收藏的存储，历史后台写入" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: 执行时写历史

会话每执行完一条语句（成功、失败、被策略拒绝、被取消）记一条历史，执行计划和等待确认而没有执行的语句不记。行数为增删改的影响行数，或查询取回的行数。`SessionManager::new` 多一个 `HistoryLog` 参数，`open` 改为接收连接配置（要用连接名和驱动）。`Service::with_parts` 启动后台写入线程，另加 `Service::with_history` 供测试指定写法。

**Files:**
- Modify: `crates/dblens-service/src/profile.rs`
- Modify: `crates/dblens-service/src/service.rs`
- Modify: `crates/dblens-service/src/session.rs`

**Interfaces:**
- Consumes：任务 2 的 `HistoryLog`、`NewHistory`、`HistoryQuery`。
- Produces：
  - `SessionManager::new(policy, metadata, history: HistoryLog)`；
  - `SessionManager::open(&Profile, conn, dialect)`（取代 `open(profile_id, environment, conn, dialect)`）；
  - `Service::with_history(store, secrets, registry, policy, history)`；
  - `profile::driver_name`、`profile::variant_code` 改为 `pub(crate)`。

- [ ] **Step 1: 修改 `crates/dblens-service/src/profile.rs`**

```diff
--- a/crates/dblens-service/src/profile.rs
+++ b/crates/dblens-service/src/profile.rs
@@ -96,7 +96,7 @@ impl Profile {
     }
 }
 
-fn driver_name(d: DriverId) -> &'static str {
+pub(crate) fn driver_name(d: DriverId) -> &'static str {
     match d {
         DriverId::Mysql => "mysql",
         DriverId::Postgres => "postgres",
@@ -137,7 +137,7 @@ const COLUMNS: &str =
     "id, name, driver, host, port, user, database, options, environment, color, connect_timeout_secs";
 
 /// 变体在库中存为它的序列化名称（如 `kingbase`）。
-fn variant_code(v: Variant) -> String {
+pub(crate) fn variant_code(v: Variant) -> String {
     serde_json::to_value(v).ok().and_then(|j| j.as_str().map(str::to_string)).unwrap_or_default()
 }
 
```

- [ ] **Step 2: 修改 `crates/dblens-service/src/service.rs`**

```diff
--- a/crates/dblens-service/src/service.rs
+++ b/crates/dblens-service/src/service.rs
@@ -5,6 +5,7 @@ use std::sync::Arc;
 
 use dblens_core::{Lang, Result, set_default_lang};
 
+use crate::history::HistoryLog;
 use crate::metadata::{MetadataCache, MetadataService};
 use crate::policy::{DefaultPolicy, Policy};
 use crate::profile::{Profile, ProfileInput};
@@ -42,16 +43,28 @@ impl Service {
         Ok(())
     }
 
-    /// 指定各个组成部分，供测试和团队版替换策略使用。
+    /// 指定各个组成部分，供测试和团队版替换策略使用。历史由后台线程写入。
     pub fn with_parts(
         store: Arc<Store>,
         secrets: Arc<Secrets>,
         registry: Arc<DriverRegistry>,
         policy: Arc<dyn Policy>,
+    ) -> Self {
+        let history = HistoryLog::start(store.clone());
+        Self::with_history(store, secrets, registry, policy, history)
+    }
+
+    /// 同 `with_parts`，另外指定历史的写法（测试中用 `HistoryLog::inline` 写完立即可查）。
+    pub fn with_history(
+        store: Arc<Store>,
+        secrets: Arc<Secrets>,
+        registry: Arc<DriverRegistry>,
+        policy: Arc<dyn Policy>,
+        history: HistoryLog,
     ) -> Self {
         let cache = Arc::new(MetadataCache::new());
         Self {
-            sessions: SessionManager::new(policy, cache.clone()),
+            sessions: SessionManager::new(policy, cache.clone(), history),
             metadata: MetadataService::new(registry.clone(), secrets.clone(), cache),
             store,
             secrets,
@@ -115,7 +128,7 @@ impl Service {
         let p = self.store.profile(profile_id)?;
         let password = self.secrets.get(profile_id)?.unwrap_or_default();
         let (conn, dialect) = self.registry.connect(&p, password).await?;
-        let id = self.sessions.open(profile_id, p.input.environment, conn, dialect)?;
+        let id = self.sessions.open(&p, conn, dialect)?;
         // 读不到当前 schema 不影响使用，工具栏显示为未选择
         let _ = self.sessions.refresh_schema(id).await;
         self.sessions.info(id)
```

- [ ] **Step 3: 修改 `crates/dblens-service/src/session.rs`**

```diff
--- a/crates/dblens-service/src/session.rs
+++ b/crates/dblens-service/src/session.rs
@@ -2,11 +2,12 @@
 //! 执行是拉取式的：调用方每调用一次 `next` 取一个事件，结果集按批读取，读多少取多少。
 //! 每条语句执行前都经过 `Policy`；需要确认时脚本停下，调用方确认后用 `confirmed = true` 重新提交。
 //! 一次请求（`ExecRequest`）可以只执行光标所在的语句、失败后继续，或只生成执行计划。
+//! 每条语句执行完（成功、失败、被拒绝或被取消）记一条历史；执行计划和等待确认而没有执行的语句不记。
 
 use std::collections::{HashMap, VecDeque};
 use std::sync::atomic::{AtomicU64, Ordering};
 use std::sync::{Arc, Mutex};
-use std::time::Instant;
+use std::time::{Instant, SystemTime, UNIX_EPOCH};
 
 use dblens_core::{Error, Msg, Result, Row, Value, msg};
 use dblens_dialect::{Dialect, SqlStatement, Variant, may_change_schema};
@@ -14,8 +15,10 @@ use dblens_driver::{CancelHandle, ColumnMeta, Connection, ExecEvent};
 use serde::{Deserialize, Serialize};
 use tokio::sync::{mpsc, oneshot};
 
+use crate::history::{HistoryLog, NewHistory};
 use crate::metadata::MetadataCache;
 use crate::policy::{Decision, ExecContext, Outcome, Policy, StatementInfo, StatementKind, classify};
+use crate::profile::{Profile, driver_name, variant_code};
 
 pub type SessionId = u64;
 
@@ -217,6 +220,7 @@ struct Handle {
 pub struct SessionManager {
     policy: Arc<dyn Policy>,
     metadata: Arc<MetadataCache>,
+    history: HistoryLog,
     next_id: AtomicU64,
     sessions: Mutex<HashMap<SessionId, Handle>>,
 }
@@ -226,8 +230,8 @@ fn gone() -> Error {
 }
 
 impl SessionManager {
-    pub fn new(policy: Arc<dyn Policy>, metadata: Arc<MetadataCache>) -> Self {
-        Self { policy, metadata, next_id: AtomicU64::new(1), sessions: Mutex::new(HashMap::new()) }
+    pub fn new(policy: Arc<dyn Policy>, metadata: Arc<MetadataCache>, history: HistoryLog) -> Self {
+        Self { policy, metadata, history, next_id: AtomicU64::new(1), sessions: Mutex::new(HashMap::new()) }
     }
 
     fn handle<T>(&self, id: SessionId, f: impl FnOnce(&Handle) -> T) -> Result<T> {
@@ -239,13 +243,8 @@ impl SessionManager {
     }
 
     /// 用一个已建立的连接创建会话（连接由调用方通过 `DriverRegistry::connect` 建立）。
-    pub fn open(
-        &self,
-        profile_id: &str,
-        environment: crate::profile::Environment,
-        conn: Box<dyn Connection>,
-        dialect: Dialect,
-    ) -> Result<SessionId> {
+    pub fn open(&self, profile: &Profile, conn: Box<dyn Connection>, dialect: Dialect) -> Result<SessionId> {
+        let profile_id = profile.id.as_str();
         let id = self.next_id.fetch_add(1, Ordering::Relaxed);
         let info = Arc::new(Mutex::new(SessionInfo {
             id,
@@ -261,9 +260,18 @@ impl SessionManager {
         let (tx, rx) = mpsc::channel(16);
         let actor = Actor {
             conn: Some(conn),
-            ctx: ExecContext { profile_id: profile_id.to_string(), environment, dialect },
+            ctx: ExecContext {
+                profile_id: profile_id.to_string(),
+                environment: profile.input.environment,
+                dialect,
+            },
+            owner: Owner {
+                name: profile.input.name.clone(),
+                driver: driver_name(profile.input.driver).to_string(),
+            },
             policy: self.policy.clone(),
             metadata: self.metadata.clone(),
+            history: self.history.clone(),
             info: info.clone(),
             cancel: cancel.clone(),
             rx,
@@ -380,13 +388,25 @@ struct Actor {
     /// 断开后仍保留（关闭时尝试回滚）；`Replace` 换成新连接。
     conn: Option<Box<dyn Connection>>,
     ctx: ExecContext,
+    owner: Owner,
     policy: Arc<dyn Policy>,
     metadata: Arc<MetadataCache>,
+    history: HistoryLog,
     info: Arc<Mutex<SessionInfo>>,
     cancel: Arc<Mutex<CancelHandle>>,
     rx: mpsc::Receiver<Command>,
 }
 
+/// 会话所属的连接配置，写历史用。
+struct Owner {
+    name: String,
+    driver: String,
+}
+
+fn now_ms() -> i64 {
+    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
+}
+
 fn reply<T>(tx: oneshot::Sender<Result<T>>, value: Result<T>) {
     // 调用方已经不等结果（如请求被中断）时丢弃
     let _ = tx.send(value);
@@ -572,10 +592,17 @@ async fn take_request(rx: &mut mpsc::Receiver<Command>, queue: &mut VecDeque<Ses
     }
 }
 
-fn convert(ev: ExecEvent, affected: &mut Option<u64>) -> SessionEvent {
+/// 驱动事件转成会话事件，同时累计影响行数和取回的行数（写历史用）。
+fn convert(ev: ExecEvent, affected: &mut Option<u64>, fetched: &mut Option<u64>) -> SessionEvent {
     match ev {
-        ExecEvent::ResultStart { columns } => SessionEvent::ResultStart { columns },
-        ExecEvent::Rows(rows) => SessionEvent::Rows { rows },
+        ExecEvent::ResultStart { columns } => {
+            fetched.get_or_insert(0);
+            SessionEvent::ResultStart { columns }
+        }
+        ExecEvent::Rows(rows) => {
+            *fetched = Some(fetched.unwrap_or(0) + rows.len() as u64);
+            SessionEvent::Rows { rows }
+        }
         ExecEvent::ResultEnd => SessionEvent::ResultEnd,
         ExecEvent::Affected(count) => {
             *affected = Some(affected.unwrap_or(0) + count);
@@ -592,13 +619,32 @@ impl Actor {
         steps: Vec<Step>,
         req: &ExecRequest,
     ) -> Option<(bool, oneshot::Sender<Result<()>>)> {
-        let Actor { conn, ctx, policy, metadata, info, rx, .. } = self;
+        let Actor { conn, ctx, owner, policy, metadata, history, info, rx, .. } = self;
         let conn = conn.as_mut()?;
         let set = |f: &dyn Fn(&mut SessionInfo)| {
             if let Ok(mut i) = info.lock() {
                 f(&mut i);
             }
         };
+        // 记一条历史。当前 schema 取执行这条语句之前的（USE 等语句执行后才更新）
+        let record =
+            |step: &Step, at: i64, elapsed_ms: Option<u64>, rows: Option<u64>, error: Option<&Error>| {
+                if req.explain {
+                    return;
+                }
+                history.record(NewHistory {
+                    at,
+                    profile_id: ctx.profile_id.clone(),
+                    profile_name: owner.name.clone(),
+                    driver: owner.driver.clone(),
+                    variant: Some(variant_code(ctx.dialect.variant())),
+                    schema: info.lock().ok().and_then(|i| i.schema.clone()),
+                    sql: step.texts[0].clone(),
+                    elapsed_ms,
+                    rows,
+                    error: error.map(|e| e.to_string()),
+                });
+            };
         let mut queue: VecDeque<SessionEvent> = VecDeque::new();
         // 一个等着数据的 Next 请求：队列空时由执行循环取数据后回复
         let mut pending: Option<(usize, oneshot::Sender<Result<Option<SessionEvent>>>)> = None;
@@ -629,13 +675,11 @@ impl Actor {
                     break 'script;
                 }
                 Decision::Deny { reason } => {
+                    let error = Error::Denied { reason };
+                    record(&step, now_ms(), None, None, Some(&error));
                     // 先发开始事件，调用方才知道被拒绝的是哪一段
                     queue.push_back(start);
-                    queue.push_back(SessionEvent::Failed {
-                        index,
-                        error: Error::Denied { reason },
-                        offset: None,
-                    });
+                    queue.push_back(SessionEvent::Failed { index, error, offset: None });
                     if req.continue_on_error {
                         continue 'script;
                     }
@@ -662,7 +706,9 @@ impl Actor {
                 }
             }
             let started = Instant::now();
+            let at = now_ms();
             let mut affected = None;
+            let mut fetched = None;
             let mut result: Result<()> = Ok(());
             for text in &step.texts {
                 result = match conn.execute(text, &[]).await {
@@ -682,7 +728,7 @@ impl Actor {
                             },
                         };
                         match stream.next(max_rows).await {
-                            Ok(Some(ev)) => reply(tx, Ok(Some(convert(ev, &mut affected)))),
+                            Ok(Some(ev)) => reply(tx, Ok(Some(convert(ev, &mut affected, &mut fetched)))),
                             Ok(None) => {
                                 pending = Some((max_rows, tx));
                                 break Ok(());
@@ -704,12 +750,11 @@ impl Actor {
                 Err(e) => Outcome::Failed { message: e.to_string() },
             };
             policy.after_execute(ctx, &stmt, &outcome).await;
+            let elapsed_ms = started.elapsed().as_millis() as u64;
+            record(&step, at, Some(elapsed_ms), affected.or(fetched), result.as_ref().err());
             match result {
                 Ok(()) => {
-                    queue.push_back(SessionEvent::StatementEnd {
-                        index,
-                        elapsed_ms: started.elapsed().as_millis() as u64,
-                    });
+                    queue.push_back(SessionEvent::StatementEnd { index, elapsed_ms });
                     if stmt.kind == StatementKind::Ddl {
                         metadata.invalidate_objects(&ctx.profile_id, &stmt.objects);
                     }
@@ -771,21 +816,84 @@ mod tests {
 
     use super::*;
     use crate::fake::{FakeConn, Log};
+    use crate::history::HistoryQuery;
     use crate::policy::DefaultPolicy;
-    use crate::profile::Environment;
+    use crate::profile::{Environment, ProfileInput};
+    use crate::store::Store;
 
     fn manager() -> (SessionManager, Arc<MetadataCache>) {
         let cache = Arc::new(MetadataCache::new());
-        (SessionManager::new(Arc::new(DefaultPolicy), cache.clone()), cache)
+        (SessionManager::new(Arc::new(DefaultPolicy), cache.clone(), HistoryLog::off()), cache)
     }
 
     fn open(m: &SessionManager, env: Environment) -> (SessionId, Log) {
         let log: Log = Arc::default();
+        let input = ProfileInput { environment: env, ..crate::profile::tests::input("开发库") };
+        let profile = Profile { id: "p1".into(), input, variant: None };
         let id =
-            m.open("p1", env, Box::new(FakeConn::new(log.clone())), Dialect::new(Variant::Postgres)).unwrap();
+            m.open(&profile, Box::new(FakeConn::new(log.clone())), Dialect::new(Variant::Postgres)).unwrap();
         (id, log)
     }
 
+    #[tokio::test]
+    async fn records_history_for_each_executed_statement() {
+        let store = Arc::new(Store::open_in_memory().unwrap());
+        let m = SessionManager::new(
+            Arc::new(DefaultPolicy),
+            Arc::new(MetadataCache::new()),
+            HistoryLog::inline(store.clone()),
+        );
+        let (id, _) = open(&m, Environment::None);
+        m.set_schema(id, "app").await.unwrap();
+        let req =
+            ExecRequest { continue_on_error: true, ..ExecRequest::new("ROWS 3; UPDATE t SET a = 1; FAIL") };
+        m.submit(id, req).await.unwrap();
+        drain(&m, id).await;
+        let items = store.history(&HistoryQuery::default()).unwrap();
+        let got: Vec<_> =
+            items.iter().map(|i| (i.entry.sql.as_str(), i.entry.rows, i.entry.error.is_some())).collect();
+        assert_eq!(
+            got,
+            [("FAIL", None, true), ("UPDATE t SET a = 1", Some(1), false), ("ROWS 3", Some(3), false)]
+        );
+        let e = &items[2].entry;
+        assert_eq!(
+            (
+                e.profile_id.as_str(),
+                e.profile_name.as_str(),
+                e.driver.as_str(),
+                e.variant.as_deref(),
+                e.schema.as_deref()
+            ),
+            ("p1", "开发库", "postgres", Some("postgres"), Some("app"))
+        );
+        assert!(e.elapsed_ms.is_some() && e.at > 0);
+
+        // 执行计划不记
+        m.submit(id, ExecRequest { explain: true, ..ExecRequest::new("ROWS 1") }).await.unwrap();
+        drain(&m, id).await;
+        assert_eq!(store.history(&HistoryQuery::default()).unwrap().len(), 3);
+    }
+
+    #[tokio::test]
+    async fn statements_waiting_for_confirmation_are_not_recorded() {
+        let store = Arc::new(Store::open_in_memory().unwrap());
+        let m = SessionManager::new(
+            Arc::new(DefaultPolicy),
+            Arc::new(MetadataCache::new()),
+            HistoryLog::inline(store.clone()),
+        );
+        let (id, _) = open(&m, Environment::Prod);
+        m.execute(id, "ROWS 1; DROP TABLE t", false).await.unwrap();
+        drain(&m, id).await;
+        let items = store.history(&HistoryQuery::default()).unwrap();
+        assert_eq!(items.iter().map(|i| i.entry.sql.as_str()).collect::<Vec<_>>(), ["ROWS 1"]);
+        // 确认后执行的会记
+        m.execute(id, "DROP TABLE t", true).await.unwrap();
+        drain(&m, id).await;
+        assert_eq!(store.history(&HistoryQuery::default()).unwrap()[0].entry.sql, "DROP TABLE t");
+    }
+
     /// 取完全部事件（每批最多 2 行）。
     async fn drain(m: &SessionManager, id: SessionId) -> Vec<SessionEvent> {
         let mut out = Vec::new();
```

- [ ] **Step 4: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（359 个，新增会话写历史 2 个）。

- [ ] **Step 5: 提交**

```bash
git add crates/dblens-service/src/profile.rs crates/dblens-service/src/service.rs crates/dblens-service/src/session.rs
git commit -m "feat(service): 每条执行过的语句写入历史（执行计划和等待确认的不记），记录行数、耗时、错误" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: 历史与收藏接口

新增 `/api/history`、`/api/favorites` 接口，接口说明补充两节。

**Files:**
- Modify: `crates/dblens-api/src/lib.rs`
- Modify: `crates/dblens-api/src/routes.rs`
- Create: `crates/dblens-api/tests/library.rs`
- Modify: `docs/notes/m0-4b-api.md`

**Interfaces:**
- Consumes：任务 2 的存储方法。
- Produces（HTTP）：
  - `GET /api/history?q=&profile=&failed=true&before=&limit=`、`DELETE /api/history/{id}`、`DELETE /api/history`；
  - `GET /api/favorites`、`POST /api/favorites`（201）、`PUT /api/favorites/{id}`、`DELETE /api/favorites/{id}`（204）。

- [ ] **Step 1: 新建 `crates/dblens-api/tests/library.rs`**

```rust
//! 历史和收藏接口（M1c1）。

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use dblens_api::{ApiConfig, router};
use dblens_service::{
    DefaultPolicy, DriverRegistry, HistoryLog, NewHistory, SecretBackend, Secrets, Service, Store,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

fn app() -> (Router, Arc<Store>) {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let secrets = Arc::new(Secrets::with_backend(store.clone(), SecretBackend::Vault));
    let registry = Arc::new(DriverRegistry::with_drivers(store.clone(), vec![]));
    let history = HistoryLog::inline(store.clone());
    let service =
        Arc::new(Service::with_history(store.clone(), secrets, registry, Arc::new(DefaultPolicy), history));
    (router(service, ApiConfig { token: "t".into(), allowed_origins: vec![] }), store)
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri).header(header::AUTHORIZATION, "Bearer t");
    if body.is_some() {
        req = req.header(header::CONTENT_TYPE, "application/json");
    }
    let req = req.body(body.map_or_else(Body::empty, |b| Body::from(b.to_string()))).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() })
}

fn entry(sql: &str, profile: &str, error: Option<&str>) -> NewHistory {
    NewHistory {
        at: 1_700_000_000_000,
        profile_id: profile.into(),
        profile_name: "开发库".into(),
        driver: "mysql".into(),
        variant: Some("mariadb".into()),
        schema: None,
        sql: sql.into(),
        elapsed_ms: Some(2),
        rows: None,
        error: error.map(str::to_string),
    }
}

fn sqls(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|i| i["sql"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn history_query_delete_and_clear() {
    let (app, store) = app();
    store
        .add_history(
            &[
                entry("SELECT 1", "p1", None),
                entry("SELECT 2", "p2", Some("已取消")),
                entry("select 3", "p1", None),
            ],
            100,
        )
        .unwrap();
    let (st, all) = call(&app, "GET", "/api/history", None).await;
    assert_eq!((st, sqls(&all)), (StatusCode::OK, vec!["select 3", "SELECT 2", "SELECT 1"]));
    assert_eq!(all[0]["profile_name"], "开发库");
    assert_eq!(all[0]["variant"], "mariadb");
    assert_eq!(all[0]["error"], Value::Null);
    let id = all[0]["id"].as_i64().unwrap();

    let q = |uri: &'static str| call(&app, "GET", uri, None);
    assert_eq!(sqls(&q("/api/history?q=SELECT%203").await.1), ["select 3"]);
    assert_eq!(sqls(&q("/api/history?profile=p2").await.1), ["SELECT 2"]);
    assert_eq!(sqls(&q("/api/history?failed=true").await.1), ["SELECT 2"]);
    assert_eq!(
        sqls(&call(&app, "GET", &format!("/api/history?before={id}&limit=1"), None).await.1),
        ["SELECT 2"]
    );

    assert_eq!(call(&app, "DELETE", &format!("/api/history/{id}"), None).await.0, StatusCode::NO_CONTENT);
    let (st, body) = call(&app, "DELETE", &format!("/api/history/{id}"), None).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
    assert_eq!(call(&app, "DELETE", "/api/history", None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(q("/api/history").await.1, json!([]));
}

#[tokio::test]
async fn favorites_crud_and_move() {
    let (app, _) = app();
    let (st, folder) =
        call(&app, "POST", "/api/favorites", Some(json!({ "kind": "folder", "name": "日报" }))).await;
    assert_eq!(
        (st, folder["kind"].as_str(), folder["parent_id"].clone()),
        (StatusCode::CREATED, Some("folder"), Value::Null)
    );
    let fid = folder["id"].as_str().unwrap().to_string();
    let (st, item) = call(
        &app,
        "POST",
        "/api/favorites",
        Some(json!({ "kind": "item", "name": "当天订单", "sql": "SELECT * FROM orders", "driver": "mysql", "parent_id": fid })),
    )
    .await;
    assert_eq!((st, item["position"].as_i64()), (StatusCode::CREATED, Some(0)));
    let iid = item["id"].as_str().unwrap().to_string();

    let (st, body) =
        call(&app, "POST", "/api/favorites", Some(json!({ "kind": "item", "name": "空" }))).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")), "收藏必须有 SQL");

    let (st, moved) = call(
        &app,
        "PUT",
        &format!("/api/favorites/{iid}"),
        Some(json!({ "name": "订单", "note": "每天看", "move": { "parent_id": null, "position": 0 } })),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (moved["name"].as_str(), moved["note"].as_str(), moved["parent_id"].clone()),
        (Some("订单"), Some("每天看"), Value::Null)
    );

    let (st, body) = call(
        &app,
        "PUT",
        &format!("/api/favorites/{fid}"),
        Some(json!({ "move": { "parent_id": fid, "position": 0 } })),
    )
    .await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid")), "不能移进自己");

    let (_, all) = call(&app, "GET", "/api/favorites", None).await;
    let names: Vec<_> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["name"].as_str().unwrap(), f["position"].as_i64().unwrap()))
        .collect();
    assert_eq!(names, [("订单", 0), ("日报", 1)]);

    assert_eq!(call(&app, "DELETE", &format!("/api/favorites/{fid}"), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "GET", "/api/favorites", None).await.1.as_array().unwrap().len(), 1);
    let (st, body) = call(&app, "DELETE", &format!("/api/favorites/{fid}"), None).await;
    assert_eq!((st, body["kind"].as_str()), (StatusCode::NOT_FOUND, Some("not_found")));
}
```

- [ ] **Step 2: 修改 `crates/dblens-api/src/lib.rs`**

```diff
--- a/crates/dblens-api/src/lib.rs
+++ b/crates/dblens-api/src/lib.rs
@@ -12,7 +12,7 @@ use std::sync::Arc;
 
 use axum::Router;
 use axum::middleware;
-use axum::routing::{get, post, put};
+use axum::routing::{delete, get, post, put};
 use dblens_service::Service;
 use tokio::net::TcpListener;
 
@@ -48,6 +48,10 @@ pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
         .route("/drivers/{driver}/lib-dir", put(routes::set_lib_dir))
         .route("/settings", get(routes::settings))
         .route("/settings/{key}", put(routes::set_setting))
+        .route("/history", get(routes::history).delete(routes::clear_history))
+        .route("/history/{id}", delete(routes::delete_history))
+        .route("/favorites", get(routes::favorites).post(routes::create_favorite))
+        .route("/favorites/{id}", put(routes::update_favorite).delete(routes::delete_favorite))
         .route("/secrets", get(routes::secret_status))
         .route("/secrets/init", post(routes::init_vault))
         .route("/secrets/unlock", post(routes::unlock))
```

- [ ] **Step 3: 修改 `crates/dblens-api/src/routes.rs`**

```diff
--- a/crates/dblens-api/src/routes.rs
+++ b/crates/dblens-api/src/routes.rs
@@ -11,7 +11,8 @@ use dblens_core::{Error, msg};
 use dblens_dialect::meta::{MetaKey, ObjectInfo, SchemaInfo};
 use dblens_driver::DriverId;
 use dblens_service::{
-    DriverStatus, Profile, ProfileInput, SecretStatus, ServerSummary, SessionId, SessionInfo,
+    DriverStatus, Favorite, FavoritePatch, HistoryItem, HistoryQuery, NewFavorite, Profile, ProfileInput,
+    SecretStatus, ServerSummary, SessionId, SessionInfo,
 };
 use serde::Deserialize;
 
@@ -86,6 +87,51 @@ pub async fn set_setting(
     Ok(StatusCode::NO_CONTENT)
 }
 
+// ---- 历史 ----
+
+pub async fn history(
+    State(s): State<AppState>,
+    Query(q): Query<HistoryQuery>,
+) -> ApiResult<Json<Vec<HistoryItem>>> {
+    Ok(Json(s.service.store.history(&q)?))
+}
+
+pub async fn delete_history(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
+    s.service.store.delete_history(id)?;
+    Ok(StatusCode::NO_CONTENT)
+}
+
+pub async fn clear_history(State(s): State<AppState>) -> ApiResult<StatusCode> {
+    s.service.store.clear_history()?;
+    Ok(StatusCode::NO_CONTENT)
+}
+
+// ---- 收藏 ----
+
+pub async fn favorites(State(s): State<AppState>) -> ApiResult<Json<Vec<Favorite>>> {
+    Ok(Json(s.service.store.favorites()?))
+}
+
+pub async fn create_favorite(
+    State(s): State<AppState>,
+    Json(b): Json<NewFavorite>,
+) -> ApiResult<(StatusCode, Json<Favorite>)> {
+    Ok((StatusCode::CREATED, Json(s.service.store.create_favorite(b)?)))
+}
+
+pub async fn update_favorite(
+    State(s): State<AppState>,
+    Path(id): Path<String>,
+    Json(b): Json<FavoritePatch>,
+) -> ApiResult<Json<Favorite>> {
+    Ok(Json(s.service.store.update_favorite(&id, b)?))
+}
+
+pub async fn delete_favorite(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
+    s.service.store.delete_favorite(&id)?;
+    Ok(StatusCode::NO_CONTENT)
+}
+
 // ---- 密码库 ----
 
 pub async fn secret_status(State(s): State<AppState>) -> ApiResult<Json<SecretStatus>> {
```

- [ ] **Step 4: 修改 `docs/notes/m0-4b-api.md`**

```diff
--- a/docs/notes/m0-4b-api.md
+++ b/docs/notes/m0-4b-api.md
@@ -111,6 +111,29 @@
 | POST | `/sessions/{id}/cancel` | 取消正在执行的语句 |
 | POST | `/sessions/{id}/schema` | 切换当前 schema，`{"schema": "…"}`（M1b1 加入） |
 
+### 历史（M1c1 加入）
+
+每条执行过的语句由服务端在执行时写入（执行计划和等待确认而没有执行的语句不记），保留条数和开关见设置 `ui.history.limit`（默认 10000）、`ui.history.enabled`（`off` 为关闭）。
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/history?q=&profile=&failed=true&before=&limit=` | 按时间倒序分页：`before` 为上一页最后一条的 `id`，`limit` 默认 100、最大 500；`q` 在 SQL 中做不区分大小写的包含匹配；`failed=true` 只返回有错误的 |
+| DELETE | `/history/{id}` | 删除一条，返回 204 |
+| DELETE | `/history` | 清空，返回 204 |
+
+每条：`{id, at, profile_id, profile_name, driver, variant, schema, sql, elapsed_ms, rows, error}`。`at` 为 Unix 毫秒；`rows` 为查询取回的行数或增删改影响的行数；`error` 为失败、被拒绝或被取消时的错误文字（写入时按默认语言），成功时为 null。
+
+### 收藏（M1c1 加入）
+
+| 方法 | 路径 | 说明 |
+|---|---|---|
+| GET | `/favorites` | 全部文件夹和收藏，平铺，按上级和次序排列 |
+| POST | `/favorites` | 新建，`{parent_id, kind: "folder" \| "item", name, sql?, note?, driver?, variant?}`，放在上级的末尾，返回 201 |
+| PUT | `/favorites/{id}` | 修改，`{name?, sql?, note?, move?: {parent_id, position}}`；`note` 为空字符串时清除；移动时插入到 `position`，同一文件夹中的其余项依次后移 |
+| DELETE | `/favorites/{id}` | 删除，文件夹连同其下内容；返回 204 |
+
+每条：`{id, parent_id, kind, name, sql, note, driver, variant, position, created_at, updated_at}`。收藏必须有 SQL，文件夹没有；上级必须是文件夹；不能把文件夹移到它自己或它的下级中（都返回 `invalid`）。
+
 ## WebSocket：`GET /api/sessions/{id}/ws?token=…&lang=…`
 
 `lang` 可选，决定事件中文字的语言（见"语言"一节）。
```

- [ ] **Step 5: 检查**

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

Expected: 全部测试通过（361 个，新增接口测试 2 个）。

- [ ] **Step 6: 提交**

```bash
git add crates/dblens-api/src/lib.rs crates/dblens-api/src/routes.rs crates/dblens-api/tests/library.rs docs/notes/m0-4b-api.md
git commit -m "feat(api): 历史与收藏接口" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: 历史侧栏与历史设置

前端接口客户端加入历史和收藏；编辑器新增"插入 SQL"通道（光标前已有内容时先换行）；`useQueryActions` 增加 `openSql`、`insertSql`。历史侧栏支持搜索、按连接和失败过滤、按日期分组、滚动加载、双击插入、右键在新标签中打开 / 复制 / 删除；执行结束后自动刷新。设置中加"执行历史"一组。

**Files:**
- Modify: `web/src/app/SettingsDialog.tsx`
- Modify: `web/src/app/Shell.tsx`
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/actions.tsx`
- Modify: `web/src/features/editor/commands.ts`
- Create: `web/src/features/history/HistoryPanel.tsx`
- Create: `web/src/features/history/history.test.ts`
- Create: `web/src/features/history/history.ts`
- Modify: `web/src/locales/en/app.json`
- Modify: `web/src/locales/zh-CN/app.json`
- Modify: `web/src/shared/api.test.ts`
- Modify: `web/src/shared/api.ts`

**Interfaces:**
- Consumes：任务 4 的接口。
- Produces：
  - `shared/api.ts`：`HistoryItem`、`HistoryParams`、`Favorite`、`FavoriteKind`、`NewFavorite`、`FavoritePatch`；`api.history / deleteHistory / clearHistory / favorites / createFavorite / updateFavorite / deleteFavorite`；
  - `features/editor/commands.ts`：`sendInsert(sql)`、`onInsert(handler)`；
  - `features/editor/actions.tsx`：`useQueryActions()` 返回 `openSql(sql, profileId?)`、`insertSql(sql, profileId?)`；
  - `features/history/history.ts`：`PAGE`、`groupByDay(items, now?)`、`firstLine(sql)`、`detail(item)`；
  - 词条 `app.history.*`、`app.settings.history*`。

- [ ] **Step 1: 新建 `web/src/features/history/history.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { HistoryItem } from '../../shared/api'
import { detail, firstLine, groupByDay } from './history'

function item(id: number, at: number, extra: Partial<HistoryItem> = {}): HistoryItem {
  return {
    id,
    at,
    profile_id: 'p1',
    profile_name: '开发库',
    driver: 'mysql',
    variant: null,
    schema: null,
    sql: `SELECT ${id}`,
    elapsed_ms: 12,
    rows: 3,
    error: null,
    ...extra,
  }
}

describe('groupByDay', () => {
  it('今天、昨天，更早的按日期', () => {
    const now = new Date(2026, 9, 26, 15, 0).getTime()
    const items = [
      item(4, new Date(2026, 9, 26, 9, 0).getTime()),
      item(3, new Date(2026, 9, 26, 0, 0).getTime()),
      item(2, new Date(2026, 9, 25, 23, 59).getTime()),
      item(1, new Date(2026, 9, 20, 8, 0).getTime()),
    ]
    const groups = groupByDay(items, now)
    expect(groups.map((g) => g.label)).toEqual(['今天', '昨天', new Date(2026, 9, 20).toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })])
    expect(groups.map((g) => g.items.map((i) => i.id))).toEqual([[4, 3], [2], [1]])
  })

  it('没有历史时为空', () => {
    expect(groupByDay([])).toEqual([])
  })
})

describe('firstLine / detail', () => {
  it('取第一行非空内容，合并空白', () => {
    expect(firstLine('\n\n  SELECT   a,\n  b FROM t')).toBe('SELECT a,')
    expect(firstLine('   ')).toBe('')
  })

  it('耗时和行数；失败的不显示行数', () => {
    const at = new Date(2026, 9, 26, 9, 5, 7).getTime()
    const time = new Date(at).toLocaleTimeString(undefined, { hour12: false })
    expect(detail(item(1, at))).toBe(`${time} · 12 ms · 3 行`)
    expect(detail(item(1, at, { error: '已取消', rows: 0 }))).toBe(`${time} · 12 ms`)
    expect(detail(item(1, at, { elapsed_ms: null, rows: null }))).toBe(time)
  })
})
```

- [ ] **Step 2: 修改 `web/src/shared/api.test.ts`**

```diff
--- a/web/src/shared/api.test.ts
+++ b/web/src/shared/api.test.ts
@@ -110,6 +110,17 @@ describe('元数据与设置', () => {
     expect(await createApi('tok', fakeFetch(200, []).fn).table('p1', 'public', 'x')).toBeNull()
   })
 
+  it('历史的查询参数；收藏的增删改', async () => {
+    const { fn, calls } = fakeFetch(200, [])
+    const api = createApi('tok', fn)
+    await api.history({ q: 'a b', failed: true, before: 5, limit: 100 })
+    await api.history()
+    expect(calls.map((c) => c.url)).toEqual(['/api/history?q=a+b&failed=true&before=5&limit=100', '/api/history'])
+    await api.updateFavorite('x/y', { move: { parent_id: null, position: 0 } })
+    expect(calls[2].url).toBe('/api/favorites/x%2Fy')
+    expect(JSON.parse(calls[2].init?.body as string)).toEqual({ move: { parent_id: null, position: 0 } })
+  })
+
   it('设置的读写', async () => {
     const { fn, calls } = fakeFetch(204, null)
     await createApi('tok', fn).putSetting('ui.theme', 'dark')
```

- [ ] **Step 3: 修改 `web/src/app/SettingsDialog.tsx`**

```diff
--- a/web/src/app/SettingsDialog.tsx
+++ b/web/src/app/SettingsDialog.tsx
@@ -1,7 +1,8 @@
-// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限、语法检查，桌面版另有"启动时自动检查更新"。
+// 设置：主题（跟随系统 / 亮色 / 暗色）、结果行数上限、语法检查、历史，桌面版另有"启动时自动检查更新"。
 // macOS 上没有页面菜单，主题只能在这里改。
 
-import { Form, InputNumber, Modal, Radio, Switch } from 'antd'
+import { useQueryClient } from '@tanstack/react-query'
+import { App, Button, Form, InputNumber, Modal, Radio, Switch } from 'antd'
 import { useTranslation } from 'react-i18next'
 
 import { DEFAULT_ROW_CAP, ROW_CAP_KEY, rowCap } from '../features/editor/runner'
@@ -10,9 +11,14 @@ import { LANG_KEY, parseLangPref } from '../shared/i18n'
 import { useSetting, useSettings } from '../shared/settings'
 import { desktop } from '../shared/shell'
 import { THEME_KEY, parseThemePref } from '../shared/theme'
+import { useApi } from './context'
 import { useUi } from './ui'
 import { useShellInfo } from './useShellInfo'
 
+/** 历史设置（由服务端读取）：值为 off 时不记录；保留条数，默认 10000。 */
+const HISTORY_ENABLED_KEY = 'ui.history.enabled'
+const HISTORY_LIMIT_KEY = 'ui.history.limit'
+
 export default function SettingsDialog() {
   const { t } = useTranslation()
   const { settingsOpen, setSettingsOpen } = useUi()
@@ -21,6 +27,24 @@ export default function SettingsDialog() {
   useSetting(ROW_CAP_KEY)
   const syntax = syntaxEnabled(useSetting(SYNTAX_KEY))
   const info = useShellInfo()
+  const api = useApi()
+  const qc = useQueryClient()
+  const { message, modal } = App.useApp()
+  const historyOn = useSetting(HISTORY_ENABLED_KEY) !== 'off'
+  const historyLimit = Number(useSetting(HISTORY_LIMIT_KEY)) || null
+  const clearHistory = () =>
+    modal.confirm({
+      title: t('app.settings.clearHistoryConfirm'),
+      okButtonProps: { danger: true },
+      onOk: async () => {
+        try {
+          await api.clearHistory()
+          await qc.invalidateQueries({ queryKey: ['history'] })
+        } catch (e) {
+          message.error((e as Error).message)
+        }
+      },
+    })
   return (
     <Modal title={t('app.settings.title')} open={settingsOpen} onCancel={() => setSettingsOpen(false)} footer={null} width={480}>
       <Form layout="horizontal" labelCol={{ span: 8 }} style={{ marginTop: 16 }}>
@@ -59,6 +83,27 @@ export default function SettingsDialog() {
         <Form.Item label={t('app.settings.syntaxCheck')} extra={t('app.settings.syntaxCheckHint')}>
           <Switch checked={syntax} onChange={(on) => useSettings.getState().set(SYNTAX_KEY, on ? null : 'off')} />
         </Form.Item>
+        <Form.Item label={t('app.settings.history')} extra={t('app.settings.historyHint')}>
+          <div className="row" style={{ gap: 8 }}>
+            <Switch
+              checked={historyOn}
+              onChange={(on) => useSettings.getState().set(HISTORY_ENABLED_KEY, on ? null : 'off')}
+            />
+            <InputNumber
+              min={100}
+              max={1_000_000}
+              step={1000}
+              value={historyLimit}
+              placeholder="10000"
+              onChange={(v) => useSettings.getState().set(HISTORY_LIMIT_KEY, v === null ? null : String(v))}
+              style={{ width: 110 }}
+            />
+            <span className="muted">{t('app.settings.historyLimitUnit')}</span>
+            <Button size="small" danger onClick={clearHistory}>
+              {t('app.settings.clearHistory')}
+            </Button>
+          </div>
+        </Form.Item>
         {desktop && (
           <Form.Item label={t('app.settings.autoCheckUpdate')}>
             <Switch
```

- [ ] **Step 4: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -11,6 +11,7 @@ import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout }
 import { useQueryActions } from '../features/editor/actions'
 import { type EditorCommand, sendCommand } from '../features/editor/commands'
 import { useTabs } from '../features/editor/tabs'
+import HistoryPanel from '../features/history/HistoryPanel'
 import ObjectTree from '../features/tree/ObjectTree'
 import { layoutStorage } from '../shared/settings'
 import StatusBar from './StatusBar'
@@ -35,6 +36,7 @@ const SIDES: { key: SideKey; icon: typeof IconDatabase }[] = [
 function SideContent({ side }: { side: SideKey }) {
   const { t } = useTranslation()
   if (side === 'db') return <ObjectTree />
+  if (side === 'history') return <HistoryPanel />
   return <Empty style={{ marginTop: 48 }} description={t('app.side.comingSoon')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
 }
 
```

- [ ] **Step 5: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -178,6 +178,85 @@ body {
   flex: 1;
   overflow: auto;
 }
+/* 历史侧栏（M1c1） */
+.hist {
+  height: 100%;
+  display: flex;
+  flex-direction: column;
+}
+.hist-filter {
+  flex: none;
+  display: flex;
+  flex-direction: column;
+  gap: 6px;
+  padding: 8px;
+  border-bottom: 1px solid var(--border);
+}
+.hist-list {
+  flex: 1;
+  overflow: auto;
+}
+.hist-empty {
+  padding: 24px 12px;
+  text-align: center;
+  display: flex;
+  flex-direction: column;
+  gap: 8px;
+  align-items: center;
+}
+.hist-day {
+  position: sticky;
+  top: 0;
+  z-index: 1;
+  padding: 4px 12px;
+  font-size: 12px;
+  color: var(--muted);
+  background: var(--panel);
+  border-bottom: 1px solid var(--border);
+}
+.hist-item {
+  padding: 6px 12px;
+  border-bottom: 1px solid var(--border);
+  cursor: default;
+}
+.hist-item:hover {
+  background: var(--hover);
+}
+.hist-sql {
+  font-family: 'JetBrains Mono', 'Cascadia Mono', Consolas, monospace;
+  font-size: 12px;
+  white-space: nowrap;
+  overflow: hidden;
+  text-overflow: ellipsis;
+}
+.hist-meta {
+  gap: 4px;
+  margin-top: 2px;
+  font-size: 11px;
+  color: var(--muted);
+}
+.hist-conn {
+  overflow: hidden;
+  text-overflow: ellipsis;
+  white-space: nowrap;
+}
+.hist-err {
+  margin-top: 2px;
+  font-size: 11px;
+  color: var(--danger);
+  white-space: nowrap;
+  overflow: hidden;
+  text-overflow: ellipsis;
+}
+.hist-preview {
+  margin: 0;
+  max-height: 320px;
+  overflow: auto;
+  white-space: pre-wrap;
+  word-break: break-all;
+  font-family: 'JetBrains Mono', 'Cascadia Mono', Consolas, monospace;
+  font-size: 12px;
+}
 .splitter {
   width: 4px;
   background: transparent;
```

- [ ] **Step 6: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -15,7 +15,7 @@ import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
 import { useSyntax } from '../sql/syntax'
 import ConfirmDialog from './ConfirmDialog'
 import Toolbar from './Toolbar'
-import { onCommand } from './commands'
+import { onCommand, onInsert } from './commands'
 import { editorLanguage, formatSql } from './dialect'
 import { errorMarks } from './markers'
 import { runnerFor, useTabRuntime } from './runner'
@@ -103,11 +103,30 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
 
   const marks = useMemo(() => errorMarks(rt.exec, rt.source), [rt.exec, rt.source])
 
-  // 执行成功的语句中有 DDL：补全用的元数据（连同后端的元数据缓存）失效，新建的表马上能补全
+  // 历史、收藏侧栏插入 SQL
+  useEffect(
+    () =>
+      onInsert((sql) => {
+        const ed = editor.current
+        const sel = ed?.getSelection()
+        const model = ed?.getModel()
+        if (!ed || !sel || !model) return
+        // 光标前已有内容时另起一行，不和原有的 SQL 连在一起
+        const before = model.getLineContent(sel.startLineNumber).slice(0, sel.startColumn - 1)
+        const text = before.trim() ? `\n${sql}` : sql
+        ed.executeEdits('insert', [{ range: sel, text, forceMoveMarkers: true }])
+        ed.focus()
+      }),
+    [],
+  )
+
+  // 执行结束：刷新历史侧栏（服务端在后台写入历史，稍等片刻）；
+  // 执行成功的语句中有 DDL 时，补全用的元数据（连同后端的元数据缓存）失效，新建的表马上能补全
   const refreshed = useRef<unknown>(null)
   useEffect(() => {
     if (rt.running || !profile || refreshed.current === rt.exec) return
     refreshed.current = rt.exec
+    if (rt.exec.statements.length) setTimeout(() => void qc.invalidateQueries({ queryKey: ['history'] }), 300)
     if (!rt.exec.statements.some((s) => !s.error && DDL.has(s.keyword))) return
     void api
       .refresh(profile.id, null)
```

- [ ] **Step 7: 修改 `web/src/features/editor/actions.tsx`**

```diff
--- a/web/src/features/editor/actions.tsx
+++ b/web/src/features/editor/actions.tsx
@@ -6,6 +6,7 @@ import { useTranslation } from 'react-i18next'
 
 import { useApi } from '../../app/context'
 import { useUi } from '../../app/ui'
+import { sendInsert } from './commands'
 import { disposeRunner, useRuntime } from './runner'
 import { useTabs } from './tabs'
 
@@ -27,6 +28,25 @@ export function useQueryActions() {
     else useUi.getState().openConnection(null)
   }
 
+  /** 用给定的 SQL 新开标签：连接已删除或没给时，依次用对象树中选中的、当前标签的、第一个连接。 */
+  const openSql = (sql: string, profileId?: string) => {
+    const { tabs, active } = useTabs.getState()
+    const exists = (id?: string | null) => (id && profiles?.some((p) => p.id === id) ? id : undefined)
+    const id =
+      exists(profileId) ??
+      exists(useUi.getState().selectedProfile) ??
+      exists(tabs.find((x) => x.id === active)?.profileId) ??
+      profiles?.[0]?.id
+    if (id) useTabs.getState().open(id, sql)
+    else useUi.getState().openConnection(null)
+  }
+
+  /** 插入到当前编辑器的光标处；没有打开的标签时新开一个。 */
+  const insertSql = (sql: string, profileId?: string) => {
+    if (useTabs.getState().active) sendInsert(sql)
+    else openSql(sql, profileId)
+  }
+
   /** 关闭标签页：有未提交的事务时先问提交还是回滚。 */
   const closeQuery = (tabId: string) => {
     const tab = useTabs.getState().tabs.find((x) => x.id === tabId)
@@ -68,5 +88,5 @@ export function useQueryActions() {
     if (active) closeQuery(active)
   }
 
-  return { newQuery, closeQuery, closeActive }
+  return { newQuery, openSql, insertSql, closeQuery, closeActive }
 }
```

- [ ] **Step 8: 修改 `web/src/features/editor/commands.ts`**

```diff
--- a/web/src/features/editor/commands.ts
+++ b/web/src/features/editor/commands.ts
@@ -8,6 +8,18 @@ export function sendCommand(cmd: EditorCommand) {
   bus.dispatchEvent(new CustomEvent<EditorCommand>('command', { detail: cmd }))
 }
 
+/** 把一段 SQL 插入当前编辑器的光标处（替换选中部分）。历史、收藏侧栏使用。 */
+export function sendInsert(sql: string) {
+  bus.dispatchEvent(new CustomEvent<string>('insert', { detail: sql }))
+}
+
+/** 订阅插入请求，返回取消订阅的函数。 */
+export function onInsert(handler: (sql: string) => void): () => void {
+  const listener = (e: Event) => handler((e as CustomEvent<string>).detail)
+  bus.addEventListener('insert', listener)
+  return () => bus.removeEventListener('insert', listener)
+}
+
 /** 订阅命令，返回取消订阅的函数。 */
 export function onCommand(handler: (cmd: EditorCommand) => void): () => void {
   const listener = (e: Event) => handler((e as CustomEvent<EditorCommand>).detail)
```

- [ ] **Step 9: 新建 `web/src/features/history/HistoryPanel.tsx`**

```tsx
// 历史侧栏（M1c1 设计文档第 6 节）：搜索、按连接和失败过滤、按日期分组、滚动加载。
// 双击插入到当前编辑器；右键在新标签中打开、复制、删除。历史由服务端在执行时写入，执行结束后自动刷新。

import { App, Button, Dropdown, Input, Select, Switch, Tooltip } from 'antd'
import { IconSearch } from '@tabler/icons-react'
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { HistoryItem } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { PAGE, detail, firstLine, groupByDay } from './history'

const ENV_COLORS: Record<string, string> = {
  dev: 'var(--env-dev)',
  test: 'var(--env-test)',
  prod: 'var(--env-prod)',
  none: 'var(--env-none)',
}

/** 输入停顿 ms 毫秒后才更新。 */
function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setV(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return v
}

export default function HistoryPanel() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { insertSql, openSql } = useQueryActions()
  const { data: profiles = [] } = useQuery({ queryKey: ['profiles'], queryFn: () => api.profiles() })
  const [text, setText] = useState('')
  const [profile, setProfile] = useState<string>('')
  const [failed, setFailed] = useState(false)
  const q = useDebounced(text.trim(), 300)

  const query = useInfiniteQuery({
    queryKey: ['history', { q, profile, failed }],
    queryFn: ({ pageParam }) =>
      api.history({ q: q || undefined, profile: profile || undefined, failed, before: pageParam, limit: PAGE }),
    initialPageParam: undefined as number | undefined,
    getNextPageParam: (last) => (last.length === PAGE ? last[last.length - 1].id : undefined),
  })
  const items = useMemo(() => query.data?.pages.flat() ?? [], [query.data])
  const groups = useMemo(() => groupByDay(items), [items])

  const remove = async (item: HistoryItem) => {
    try {
      await api.deleteHistory(item.id)
      await qc.invalidateQueries({ queryKey: ['history'] })
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const menu = (item: HistoryItem) => ({
    items: [
      { key: 'open', label: t('app.history.openInTab') },
      { key: 'copy', label: t('app.history.copy') },
      { type: 'divider' as const },
      { key: 'delete', label: t('app.history.delete'), danger: true },
    ],
    onClick: ({ key }: { key: string }) => {
      if (key === 'open') openSql(item.sql, item.profile_id)
      else if (key === 'copy') void navigator.clipboard.writeText(item.sql)
      else if (key === 'delete') void remove(item)
    },
  })

  return (
    <div className="hist">
      <div className="hist-filter">
        <Input
          size="small"
          allowClear
          prefix={<IconSearch size={14} />}
          placeholder={t('app.history.search')}
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <div className="row" style={{ gap: 8 }}>
          <Select
            size="small"
            className="grow"
            value={profile}
            onChange={setProfile}
            popupMatchSelectWidth={false}
            options={[
              { value: '', label: t('app.history.allConnections') },
              ...profiles.map((p) => ({ value: p.id, label: p.name })),
            ]}
          />
          <span className="row" style={{ gap: 4 }}>
            <Switch size="small" checked={failed} onChange={setFailed} />
            {t('app.history.failedOnly')}
          </span>
        </div>
      </div>
      <div
        className="hist-list"
        onScroll={(e) => {
          const el = e.currentTarget
          if (el.scrollTop + el.clientHeight > el.scrollHeight - 80 && query.hasNextPage && !query.isFetchingNextPage) {
            void query.fetchNextPage()
          }
        }}
      >
        {query.error ? (
          <div className="hist-empty">
            <div className="err">{(query.error as Error).message}</div>
            <Button size="small" onClick={() => void query.refetch()}>
              {t('app.history.retry')}
            </Button>
          </div>
        ) : !query.isPending && items.length === 0 ? (
          <div className="hist-empty muted">{t('app.history.empty')}</div>
        ) : (
          groups.map((g) => (
            <div key={g.day}>
              <div className="hist-day">{g.label}</div>
              {g.items.map((item) => {
                const p = profiles.find((x) => x.id === item.profile_id)
                return (
                  <Dropdown key={item.id} trigger={['contextMenu']} menu={menu(item)}>
                    <Tooltip
                      placement="right"
                      mouseEnterDelay={0.6}
                      title={<pre className="hist-preview">{item.sql}</pre>}
                      styles={{ root: { maxWidth: 560 } }}
                    >
                      <div className="hist-item" onDoubleClick={() => insertSql(item.sql, item.profile_id)}>
                        <div className="hist-sql">{firstLine(item.sql)}</div>
                        <div className="row hist-meta">
                          <span className="dot" style={{ background: ENV_COLORS[p?.environment ?? 'none'] }} />
                          <DbIcon driver={item.driver} variant={item.variant} />
                          <span className="hist-conn">{item.profile_name}</span>
                          <span className="grow" />
                          <span>{detail(item)}</span>
                        </div>
                        {item.error && <div className="hist-err">{item.error}</div>}
                      </div>
                    </Tooltip>
                  </Dropdown>
                )
              })}
            </div>
          ))
        )}
      </div>
    </div>
  )
}
```

- [ ] **Step 10: 新建 `web/src/features/history/history.ts`**

```ts
// 历史侧栏的纯逻辑（M1c1 设计文档第 6 节）：按日期分组、SQL 摘要、说明文字。

import type { HistoryItem } from '../../shared/api'
import { t } from '../../shared/i18n'

/** 每页条数。 */
export const PAGE = 100

export interface DayGroup {
  /** 当天 0 点（本地时间）的时间戳，用作 key。 */
  day: number
  label: string
  items: HistoryItem[]
}

function startOfDay(ms: number): number {
  const d = new Date(ms)
  d.setHours(0, 0, 0, 0)
  return d.getTime()
}

/** 按本地日期分组：今天、昨天，更早的显示日期。列表本身已按时间倒序。 */
export function groupByDay(items: HistoryItem[], now = Date.now()): DayGroup[] {
  const today = startOfDay(now)
  const yesterday = startOfDay(today - 1)
  const out: DayGroup[] = []
  for (const item of items) {
    const day = startOfDay(item.at)
    let g = out[out.length - 1]
    if (!g || g.day !== day) {
      const label =
        day === today
          ? t('app.history.today')
          : day === yesterday
            ? t('app.history.yesterday')
            : new Date(day).toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })
      g = { day, label, items: [] }
      out.push(g)
    }
    g.items.push(item)
  }
  return out
}

/** SQL 的摘要：第一行非空内容，连续空白合成一个。 */
export function firstLine(sql: string): string {
  const line = sql.split('\n').find((l) => l.trim()) ?? ''
  return line.trim().replace(/\s+/g, ' ')
}

/** 时间（时:分:秒）、耗时、行数。 */
export function detail(item: HistoryItem): string {
  const time = new Date(item.at).toLocaleTimeString(undefined, { hour12: false })
  const parts = [time]
  if (item.elapsed_ms !== null) parts.push(t('app.history.elapsed', { ms: item.elapsed_ms }))
  if (item.rows !== null && !item.error) parts.push(t('app.history.rows', { count: item.rows }))
  return parts.join(' · ')
}
```

- [ ] **Step 11: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -13,7 +13,12 @@
     "rowCapHint": "Stop reading a result set after this many rows",
     "autoCheckUpdate": "Check for updates at startup",
     "syntaxCheck": "Syntax check",
-    "syntaxCheckHint": "Mark syntax problems while editing; databases compatible with MySQL / PostgreSQL / Oracle get yellow hints only. Does not affect execution"
+    "syntaxCheckHint": "Mark syntax problems while editing; databases compatible with MySQL / PostgreSQL / Oracle get yellow hints only. Does not affect execution",
+    "history": "Query history",
+    "historyHint": "Records every executed statement on this computer only; the oldest are removed beyond the limit",
+    "historyLimitUnit": "entries",
+    "clearHistory": "Clear History",
+    "clearHistoryConfirm": "Clear all query history? This cannot be undone."
   },
   "theme": {
     "system": "System",
@@ -91,5 +96,20 @@
     "socketFailed": "WebSocket connection failed",
     "lobTruncated": "{{preview}}… ({{total}} total)",
     "lobTruncatedBinary": "(binary) {{preview}}… ({{total}} total)"
+  },
+  "history": {
+    "search": "Search SQL",
+    "allConnections": "All connections",
+    "failedOnly": "Failed only",
+    "today": "Today",
+    "yesterday": "Yesterday",
+    "elapsed": "{{ms}} ms",
+    "rows_one": "{{count}} row",
+    "rows_other": "{{count}} rows",
+    "empty": "No query history yet",
+    "retry": "Retry",
+    "openInTab": "Open in New Tab",
+    "copy": "Copy SQL",
+    "delete": "Delete"
   }
 }
```

- [ ] **Step 12: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -13,7 +13,12 @@
     "rowCapHint": "一个结果集读到这么多行后停止读取",
     "autoCheckUpdate": "启动时自动检查更新",
     "syntaxCheck": "语法检查",
-    "syntaxCheckHint": "编辑时标出语法问题；兼容 MySQL / PostgreSQL / Oracle 的国产库只标黄色提示。不影响执行"
+    "syntaxCheckHint": "编辑时标出语法问题；兼容 MySQL / PostgreSQL / Oracle 的国产库只标黄色提示。不影响执行",
+    "history": "执行历史",
+    "historyHint": "记录每条执行过的语句，只保存在本机；超过条数时删除最旧的",
+    "historyLimitUnit": "条",
+    "clearHistory": "清空历史",
+    "clearHistoryConfirm": "清空全部执行历史？此操作不能撤销。"
   },
   "theme": {
     "system": "跟随系统",
@@ -90,5 +95,19 @@
     "socketFailed": "WebSocket 连接失败",
     "lobTruncated": "{{preview}}…（共 {{total}}）",
     "lobTruncatedBinary": "(二进制) {{preview}}…（共 {{total}}）"
+  },
+  "history": {
+    "search": "搜索 SQL",
+    "allConnections": "全部连接",
+    "failedOnly": "只看失败",
+    "today": "今天",
+    "yesterday": "昨天",
+    "elapsed": "{{ms}} ms",
+    "rows": "{{count}} 行",
+    "empty": "还没有执行历史",
+    "retry": "重试",
+    "openInTab": "在新标签中打开",
+    "copy": "复制 SQL",
+    "delete": "删除"
   }
 }
```

- [ ] **Step 13: 修改 `web/src/shared/api.ts`**

```diff
--- a/web/src/shared/api.ts
+++ b/web/src/shared/api.ts
@@ -165,6 +165,70 @@ export interface SynonymInfo {
   db_link: string | null
 }
 
+/** 一条执行历史（M1c1）。 */
+export interface HistoryItem {
+  id: number
+  /** 执行开始时间，Unix 毫秒。 */
+  at: number
+  profile_id: string
+  /** 执行时的连接名（连接删除后仍能显示）。 */
+  profile_name: string
+  driver: DriverId
+  variant: string | null
+  schema: string | null
+  sql: string
+  elapsed_ms: number | null
+  /** 查询取回的行数，或增删改影响的行数。 */
+  rows: number | null
+  /** 失败、被拒绝或被取消时的错误文字；成功时为 null。 */
+  error: string | null
+}
+
+export interface HistoryParams {
+  q?: string
+  profile?: string
+  failed?: boolean
+  /** 上一页最后一条的 id。 */
+  before?: number
+  limit?: number
+}
+
+export type FavoriteKind = 'folder' | 'item'
+
+/** 收藏的文件夹或 SQL（M1c1）。 */
+export interface Favorite {
+  id: string
+  parent_id: string | null
+  kind: FavoriteKind
+  name: string
+  sql: string | null
+  note: string | null
+  driver: DriverId | null
+  variant: string | null
+  /** 同一文件夹内的次序。 */
+  position: number
+  created_at: number
+  updated_at: number
+}
+
+export interface NewFavorite {
+  parent_id: string | null
+  kind: FavoriteKind
+  name: string
+  sql?: string
+  note?: string
+  driver?: DriverId
+  variant?: string | null
+}
+
+export interface FavoritePatch {
+  name?: string
+  sql?: string
+  /** 空字符串清除说明。 */
+  note?: string
+  move?: { parent_id: string | null; position: number }
+}
+
 export type FetchFn = (input: string, init?: RequestInit) => Promise<Response>
 
 export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o)) {
@@ -221,6 +285,24 @@ export function createApi(token: string, fetchFn: FetchFn = (i, o) => fetch(i, o
     synonyms: (id: string, schema: string) => request<SynonymInfo[]>('GET', `${s(id, schema)}/synonyms`),
     /** 清掉后端的元数据缓存；不给 schema 时清整个连接的。 */
     refresh: (id: string, schema: string | null) => request<null>('POST', `${p(id)}/refresh`, { schema }),
+    history: (p: HistoryParams = {}) =>
+      request<HistoryItem[]>(
+        'GET',
+        `/history${q({
+          q: p.q,
+          profile: p.profile,
+          failed: p.failed ? 'true' : undefined,
+          before: p.before === undefined ? undefined : String(p.before),
+          limit: p.limit === undefined ? undefined : String(p.limit),
+        })}`,
+      ),
+    deleteHistory: (id: number) => request<null>('DELETE', `/history/${id}`),
+    clearHistory: () => request<null>('DELETE', '/history'),
+    favorites: () => request<Favorite[]>('GET', '/favorites'),
+    createFavorite: (f: NewFavorite) => request<Favorite>('POST', '/favorites', f),
+    updateFavorite: (id: string, patch: FavoritePatch) =>
+      request<Favorite>('PUT', `/favorites/${encodeURIComponent(id)}`, patch),
+    deleteFavorite: (id: string) => request<null>('DELETE', `/favorites/${encodeURIComponent(id)}`),
     openSession: (profileId: string) => request<SessionInfo>('POST', '/sessions', { profile_id: profileId }),
     session: (id: number) => request<SessionInfo>('GET', `/sessions/${id}`),
     /** 关闭会话；有未提交的修改时按 commit 提交或回滚。 */
```

- [ ] **Step 14: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；28 个测试文件、181 个测试通过；打包成功。

- [ ] **Step 15: 提交**

```bash
git add web/src/app/SettingsDialog.tsx web/src/app/Shell.tsx web/src/app/shell.css web/src/features/editor/QueryView.tsx web/src/features/editor/actions.tsx web/src/features/editor/commands.ts web/src/features/history/HistoryPanel.tsx web/src/features/history/history.test.ts web/src/features/history/history.ts web/src/locales/en/app.json web/src/locales/zh-CN/app.json web/src/shared/api.test.ts web/src/shared/api.ts
git commit -m "feat(web): 历史侧栏：搜索、按连接和失败过滤、按日期分组、插入或在新标签中打开；设置中的历史开关、条数和清空" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: 收藏

收藏侧栏：多级文件夹，拖动整理，双击插入，右键在新标签中打开 / 编辑 / 新建子文件夹 / 删除（文件夹删除前确认）。收藏对话框可新建收藏、新建文件夹、编辑；编辑时也可以换文件夹（移到末尾），不用拖动也能整理。Ctrl+D、工具栏星标、SQL 菜单、历史右键菜单都能打开收藏对话框：有选中时收藏选中部分，否则收藏光标所在的语句（SQL 语言 worker 新增 `statement` 请求，按方言分句）。

**Files:**
- Modify: `web/src/app/Shell.tsx`
- Modify: `web/src/app/menus.test.ts`
- Modify: `web/src/app/menus.ts`
- Modify: `web/src/app/shell.css`
- Modify: `web/src/features/editor/QueryView.tsx`
- Modify: `web/src/features/editor/SqlEditor.tsx`
- Modify: `web/src/features/editor/Toolbar.tsx`
- Modify: `web/src/features/editor/commands.ts`
- Create: `web/src/features/favorites/FavoriteDialog.tsx`
- Create: `web/src/features/favorites/FavoritesPanel.tsx`
- Create: `web/src/features/favorites/dialog.ts`
- Create: `web/src/features/favorites/tree.test.ts`
- Create: `web/src/features/favorites/tree.ts`
- Modify: `web/src/features/history/HistoryPanel.tsx`
- Modify: `web/src/features/sql/client.ts`
- Modify: `web/src/features/sql/dtsql.test.ts`
- Modify: `web/src/features/sql/dtsql.ts`
- Modify: `web/src/features/sql/plsql/parser.test.ts`
- Modify: `web/src/features/sql/plsql/parser.ts`
- Modify: `web/src/features/sql/protocol.ts`
- Modify: `web/src/features/sql/types.ts`
- Modify: `web/src/features/sql/worker.ts`
- Modify: `web/src/locales/en/app.json`
- Modify: `web/src/locales/en/editor.json`
- Modify: `web/src/locales/zh-CN/app.json`
- Modify: `web/src/locales/zh-CN/editor.json`

**Interfaces:**
- Consumes：任务 5 的接口客户端、`openSql`、`insertSql`；M1b2 的 SQL 语言 worker。
- Produces：
  - `features/sql/types.ts`：`StatementRange`、`pickStatement(ranges, offset)`；`SqlParser.statementAt(sql, offset)`；`SqlClient.statementAt(dialect, sql, offset)`；
  - `features/favorites/tree.ts`：`buildTree`、`canMoveInto`、`dropTarget(favs, dragId, targetId, -1 | 0 | 1)`、`defaultName`、`folderPath`；
  - `features/favorites/dialog.ts`：`useFavoriteDialog`、`FavoriteDraft`、`LAST_FOLDER_KEY = 'ui.favorites.last-folder'`；
  - 编辑器命令 `favorite`（Ctrl+D）；词条 `app.favorites.*`、`app.history.addFavorite`、`app.menu.sql.favorite`、`editor.command.favorite`、`editor.toolbar.favorite`。

- [ ] **Step 1: 修改 `web/src/app/menus.test.ts`**

```diff
--- a/web/src/app/menus.test.ts
+++ b/web/src/app/menus.test.ts
@@ -11,7 +11,7 @@ describe('标题栏菜单', () => {
     const m = buildMenus(base)
     expect(m.map((g) => g.label)).toEqual(['文件', '查看', 'SQL', '窗口', '帮助'])
     expect(keys(m[0].items)).toEqual(['new-connection', 'new-query', 'close-tab', '-', 'settings', '-', 'quit'])
-    expect(keys(m[2].items)).toEqual(['sql:run-current', 'sql:run-all', 'sql:cancel', '-', 'sql:format', 'sql:explain'])
+    expect(keys(m[2].items)).toEqual(['sql:run-current', 'sql:run-all', 'sql:cancel', '-', 'sql:format', 'sql:explain', '-', 'sql:favorite'])
     expect(keys(m[4].items)).toEqual(['help:check-update', 'help:auto-check', '-', 'help:about'])
   })
 
```

- [ ] **Step 2: 新建 `web/src/features/favorites/tree.test.ts`**

```ts
import { describe, expect, it } from 'vitest'

import type { Favorite } from '../../shared/api'
import { buildTree, canMoveInto, defaultName, dropTarget, folderPath } from './tree'

function fav(id: string, parent: string | null, position: number, kind: Favorite['kind'] = 'item'): Favorite {
  return {
    id,
    parent_id: parent,
    kind,
    name: id,
    sql: kind === 'item' ? `SELECT '${id}'` : null,
    note: null,
    driver: null,
    variant: null,
    position,
    created_at: 0,
    updated_at: 0,
  }
}

// 根：a、F（文件夹，含 b、G（文件夹，含 c））、d
const FAVS = [
  fav('d', null, 2),
  fav('a', null, 0),
  fav('F', null, 1, 'folder'),
  fav('G', 'F', 1, 'folder'),
  fav('b', 'F', 0),
  fav('c', 'G', 0),
]

describe('buildTree', () => {
  it('按次序组成树', () => {
    const names = (nodes: ReturnType<typeof buildTree>): unknown[] =>
      nodes.map((n) => (n.children.length ? [n.fav.id, names(n.children)] : n.fav.id))
    expect(names(buildTree(FAVS))).toEqual(['a', ['F', ['b', ['G', ['c']]]], 'd'])
  })
})

describe('canMoveInto', () => {
  it('只能移到文件夹，且不能是自己或下级', () => {
    expect(canMoveInto(FAVS, 'a', 'F')).toBe(true)
    expect(canMoveInto(FAVS, 'a', null)).toBe(true)
    expect(canMoveInto(FAVS, 'a', 'b')).toBe(false)
    expect(canMoveInto(FAVS, 'F', 'F')).toBe(false)
    expect(canMoveInto(FAVS, 'F', 'G')).toBe(false)
    expect(canMoveInto(FAVS, 'G', null)).toBe(true)
  })
})

describe('dropTarget', () => {
  it('拖到前后：同级中的位置不算被拖动项', () => {
    expect(dropTarget(FAVS, 'a', 'd', 1)).toEqual({ parent_id: null, position: 2 })
    expect(dropTarget(FAVS, 'd', 'a', -1)).toEqual({ parent_id: null, position: 0 })
    expect(dropTarget(FAVS, 'a', 'b', -1)).toEqual({ parent_id: 'F', position: 0 })
    expect(dropTarget(FAVS, 'c', 'G', 1)).toEqual({ parent_id: 'F', position: 2 })
  })

  it('拖到文件夹里面放在最前；不能拖进自己的下级', () => {
    expect(dropTarget(FAVS, 'a', 'G', 0)).toEqual({ parent_id: 'G', position: 0 })
    expect(dropTarget(FAVS, 'a', 'b', 0)).toBeNull()
    expect(dropTarget(FAVS, 'F', 'G', 0)).toBeNull()
    expect(dropTarget(FAVS, 'F', 'c', 1)).toBeNull()
    expect(dropTarget(FAVS, 'a', 'a', 1)).toBeNull()
  })
})

describe('defaultName / folderPath', () => {
  it('合并换行和空白，过长时截断', () => {
    expect(defaultName('\n  SELECT  *\n FROM t')).toBe('SELECT * FROM t')
    const long = `SELECT ${'x'.repeat(80)}`
    expect(defaultName(long)).toHaveLength(60)
    expect(defaultName(long).endsWith('…')).toBe(true)
  })

  it('文件夹路径', () => {
    expect(folderPath(FAVS, 'G')).toEqual(['F', 'G'])
  })
})
```

- [ ] **Step 3: 修改 `web/src/features/sql/dtsql.test.ts`**

```diff
--- a/web/src/features/sql/dtsql.test.ts
+++ b/web/src/features/sql/dtsql.test.ts
@@ -59,6 +59,18 @@ describe('dt-sql-parser 适配', () => {
     expect(s.want).toContain('column')
   })
 
+  it('光标所在的语句', () => {
+    const sql = 'SELECT 1;\nSELECT 2 FROM t;\n\nUPDATE t SET a = 1'
+    const at = (o: number) => {
+      const r = mysql.statementAt(sql, o)
+      return r && sql.slice(r.start, r.end)
+    }
+    expect(at(0)).toBe('SELECT 1')
+    expect(at(12)).toBe('SELECT 2 FROM t')
+    expect(at(sql.length)).toBe('UPDATE t SET a = 1')
+    expect(postgres.statementAt('  ', 0)).toBeNull()
+  })
+
   it('反引号名称去掉引号', () => {
     expect(at(mysql, 'SELECT t.| FROM `my db`.`t 1` t').tables).toEqual([{ schema: 'my db', name: 't 1', alias: 't' }])
   })
```

- [ ] **Step 4: 修改 `web/src/features/sql/plsql/parser.test.ts`**

```diff
--- a/web/src/features/sql/plsql/parser.test.ts
+++ b/web/src/features/sql/plsql/parser.test.ts
@@ -75,6 +75,20 @@ SELECT 1 FROM dual`
   })
 })
 
+describe('PL/SQL 光标所在的语句', () => {
+  it('PL/SQL 块整体算一条，不含结束符', () => {
+    const sql = 'SELECT 1 FROM dual;\nBEGIN\n  NULL;\nEND;\n/\nSELECT 2 FROM dual  '
+    const at = (o: number) => {
+      const r = plsql.statementAt(sql, o)
+      return r && sql.slice(r.start, r.end)
+    }
+    expect(at(3)).toBe('SELECT 1 FROM dual')
+    expect(at(sql.indexOf('NULL'))).toBe('BEGIN\n  NULL;\nEND;')
+    expect(at(sql.length)).toBe('SELECT 2 FROM dual')
+    expect(plsql.statementAt('', 0)).toBeNull()
+  })
+})
+
 describe('PL/SQL 补全分析', () => {
   it.each([
     ['SELECT * FROM |', ['table', 'view']],
```

- [ ] **Step 5: 修改 `web/src/app/Shell.tsx`**

```diff
--- a/web/src/app/Shell.tsx
+++ b/web/src/app/Shell.tsx
@@ -11,6 +11,8 @@ import { Group, Panel, type PanelImperativeHandle, Separator, useDefaultLayout }
 import { useQueryActions } from '../features/editor/actions'
 import { type EditorCommand, sendCommand } from '../features/editor/commands'
 import { useTabs } from '../features/editor/tabs'
+import FavoriteDialog from '../features/favorites/FavoriteDialog'
+import FavoritesPanel from '../features/favorites/FavoritesPanel'
 import HistoryPanel from '../features/history/HistoryPanel'
 import ObjectTree from '../features/tree/ObjectTree'
 import { layoutStorage } from '../shared/settings'
@@ -37,6 +39,7 @@ function SideContent({ side }: { side: SideKey }) {
   const { t } = useTranslation()
   if (side === 'db') return <ObjectTree />
   if (side === 'history') return <HistoryPanel />
+  if (side === 'favorites') return <FavoritesPanel />
   return <Empty style={{ marginTop: 48 }} description={t('app.side.comingSoon')} image={Empty.PRESENTED_IMAGE_SIMPLE} />
 }
 
@@ -72,6 +75,7 @@ function shortcut(e: KeyboardEvent): EditorCommand | 'new-connection' | 'new-que
   if (!ctrl && !e.altKey && key === 'f5') return 'run-all'
   if ((ctrl && key === 'pause') || (e.metaKey && key === '.')) return 'cancel'
   if (e.shiftKey && e.altKey && key === 'f') return 'format'
+  if (ctrl && !e.shiftKey && !e.altKey && key === 'd') return 'favorite'
   return null
 }
 
@@ -150,6 +154,7 @@ export default function Shell() {
         </Group>
       </div>
       <StatusBar />
+      <FavoriteDialog />
     </div>
   )
 }
```

- [ ] **Step 6: 修改 `web/src/app/menus.ts`**

```diff
--- a/web/src/app/menus.ts
+++ b/web/src/app/menus.ts
@@ -52,6 +52,8 @@ export function buildMenus(s: MenuState): MenuGroup[] {
     'divider',
     { key: 'sql:format', label: t('app.menu.sql.format'), shortcut: 'Shift+Alt+F' },
     { key: 'sql:explain', label: t('app.menu.sql.explain') },
+    'divider',
+    { key: 'sql:favorite', label: t('app.menu.sql.favorite'), shortcut: 'Ctrl+D' },
   ]
 
   const groups: MenuGroup[] = [
```

- [ ] **Step 7: 修改 `web/src/app/shell.css`**

```diff
--- a/web/src/app/shell.css
+++ b/web/src/app/shell.css
@@ -248,6 +248,44 @@ body {
   overflow: hidden;
   text-overflow: ellipsis;
 }
+/* 收藏侧栏（M1c1） */
+.fav {
+  height: 100%;
+  display: flex;
+  flex-direction: column;
+}
+.fav-bar {
+  flex: none;
+  gap: 4px;
+  padding: 4px 6px 4px 12px;
+  border-bottom: 1px solid var(--border);
+  font-size: 12px;
+}
+.fav-tree {
+  flex: 1;
+  overflow: auto;
+  padding: 4px 0;
+  background: transparent;
+}
+.fav-tree .ant-tree-node-content-wrapper {
+  display: flex;
+  align-items: center;
+  min-width: 0;
+}
+.fav-tree .ant-tree-title {
+  flex: 1;
+  min-width: 0;
+}
+.fav-title {
+  display: block;
+  overflow: hidden;
+  text-overflow: ellipsis;
+  white-space: nowrap;
+}
+.mono {
+  font-family: 'JetBrains Mono', 'Cascadia Mono', Consolas, monospace;
+  font-size: 12px;
+}
 .hist-preview {
   margin: 0;
   max-height: 320px;
```

- [ ] **Step 8: 修改 `web/src/features/editor/QueryView.tsx`**

```diff
--- a/web/src/features/editor/QueryView.tsx
+++ b/web/src/features/editor/QueryView.tsx
@@ -11,7 +11,10 @@ import { useApi } from '../../app/context'
 import { type Profile, errorText } from '../../shared/api'
 import { layoutStorage } from '../../shared/settings'
 import ResultsPanel from '../results/ResultsPanel'
+import { useFavoriteDialog } from '../favorites/dialog'
+import { sqlDialect } from '../sql/dialects'
 import { defaultSchema, metaKey, metaReader } from '../sql/metadata'
+import { sqlClient } from '../sql/service'
 import { useSyntax } from '../sql/syntax'
 import ConfirmDialog from './ConfirmDialog'
 import Toolbar from './Toolbar'
@@ -52,6 +55,7 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
             ? { sql: model.getValueInRange(selected), base: model.getOffsetAt(selected.getStartPosition()) }
             : null
         const cursor = model.getOffsetAt(ed.getPosition() ?? { lineNumber: 1, column: 1 })
+        const variant = rt.session?.variant ?? profile?.variant ?? null
         switch (cmd) {
           case 'run-current':
           case 'explain': {
@@ -69,6 +73,24 @@ export default function QueryView({ tab, profiles }: { tab: QueryTab; profiles:
           case 'cancel':
             runner.cancel()
             break
+          case 'favorite': {
+            // 收藏选中的部分，没有选中时收藏光标所在的语句（解析器不可用时收藏全文）
+            const text = selected ? model.getValueInRange(selected) : null
+            void (async () => {
+              let sql = text
+              if (!sql) {
+                const all = model.getValue()
+                const r = profile
+                  ? await sqlClient.statementAt(sqlDialect(profile.driver, variant).dialect, all, cursor)
+                  : null
+                sql = r?.ok && r.value ? all.slice(r.value.start, r.value.end) : all
+              }
+              sql = sql.trim()
+              if (!sql) message.info(t('app.favorites.nothing'))
+              else useFavoriteDialog.getState().open({ mode: 'create', kind: 'item', sql, driver: profile?.driver, variant })
+            })()
+            break
+          }
           case 'first-problem': {
             const first = useSyntax.getState().tabs[tab.id]?.first
             if (first) {
```

- [ ] **Step 9: 修改 `web/src/features/editor/SqlEditor.tsx`**

```diff
--- a/web/src/features/editor/SqlEditor.tsx
+++ b/web/src/features/editor/SqlEditor.tsx
@@ -20,7 +20,7 @@ export type MonacoEditor = Parameters<OnMount>[0]
 registerSqlProviders()
 
 const { KeyMod, KeyCode } = monaco
-type CommandLabel = `editor.command.${'runCurrent' | 'runAll' | 'stop' | 'format' | 'explain'}`
+type CommandLabel = `editor.command.${'runCurrent' | 'runAll' | 'stop' | 'format' | 'explain' | 'favorite'}`
 // label 是词条键，注册动作时按当前界面语言翻译
 const ACTIONS: { id: EditorCommand; label: CommandLabel; keys: number[] }[] = [
   { id: 'run-current', label: 'editor.command.runCurrent', keys: [KeyMod.CtrlCmd | KeyCode.Enter] },
@@ -28,6 +28,8 @@ const ACTIONS: { id: EditorCommand; label: CommandLabel; keys: number[] }[] = [
   { id: 'cancel', label: 'editor.command.stop', keys: [KeyMod.CtrlCmd | KeyCode.PauseBreak, KeyMod.CtrlCmd | KeyCode.Period] },
   { id: 'format', label: 'editor.command.format', keys: [KeyMod.Shift | KeyMod.Alt | KeyCode.KeyF] },
   { id: 'explain', label: 'editor.command.explain', keys: [] },
+  // 覆盖 Monaco 默认的 Ctrl+D（选中下一个相同的词）
+  { id: 'favorite', label: 'editor.command.favorite', keys: [KeyMod.CtrlCmd | KeyCode.KeyD] },
 ]
 
 export interface SqlEditorProps {
```

- [ ] **Step 10: 修改 `web/src/features/editor/Toolbar.tsx`**

```diff
--- a/web/src/features/editor/Toolbar.tsx
+++ b/web/src/features/editor/Toolbar.tsx
@@ -1,5 +1,5 @@
 // 编辑器工具栏（设计稿第 2 部分）：执行当前语句、执行全部、停止、出错停止 / 继续；连接和 schema；
-// 事务模式、提交、回滚；格式化、执行计划。会话级操作出错时用全局提示显示。
+// 事务模式、提交、回滚；收藏、格式化、执行计划。会话级操作出错时用全局提示显示。
 
 import { App, Button, Select, Tooltip } from 'antd'
 import {
@@ -10,6 +10,7 @@ import {
   IconPlayerPlay,
   IconPlayerStop,
   IconPlayerTrackNext,
+  IconStar,
   IconWand,
 } from '@tabler/icons-react'
 import { useQuery } from '@tanstack/react-query'
@@ -152,6 +153,9 @@ export default function Toolbar({ tab, rt, runner, profiles }: ToolbarProps) {
         </span>
       )}
       <span className="grow" />
+      <IconBtn title={t('editor.toolbar.favorite')} onClick={() => sendCommand('favorite')}>
+        <IconStar size={16} />
+      </IconBtn>
       <IconBtn title={t('editor.toolbar.format')} onClick={() => sendCommand('format')}>
         <IconWand size={16} />
       </IconBtn>
```

- [ ] **Step 11: 修改 `web/src/features/editor/commands.ts`**

```diff
--- a/web/src/features/editor/commands.ts
+++ b/web/src/features/editor/commands.ts
@@ -1,6 +1,6 @@
 // 编辑器命令：菜单、全局快捷键和工具栏都发命令，由当前标签页的视图执行（它持有编辑器实例）。
 
-export type EditorCommand = 'run-current' | 'run-all' | 'cancel' | 'format' | 'explain' | 'first-problem'
+export type EditorCommand = 'run-current' | 'run-all' | 'cancel' | 'format' | 'explain' | 'first-problem' | 'favorite'
 
 const bus = new EventTarget()
 
```

- [ ] **Step 12: 新建 `web/src/features/favorites/FavoriteDialog.tsx`**

```tsx
// 收藏对话框（M1c1 设计文档第 6 节）：收藏 SQL（名称默认取 SQL 开头、选文件夹、说明）、新建文件夹、编辑。
// 编辑时也可以换文件夹（移到末尾），不用拖动也能整理。

import { App, Form, Input, Modal, TreeSelect } from 'antd'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import type { Favorite } from '../../shared/api'
import { useSettings } from '../../shared/settings'
import { LAST_FOLDER_KEY, useFavoriteDialog } from './dialog'
import { type FavNode, buildTree, canMoveInto, defaultName } from './tree'

interface Values {
  name: string
  folder: string
  sql?: string
  note?: string
}

/** 文件夹选择框中根节点的值（TreeSelect 把 null 和空字符串当作没有选择）。 */
const ROOT = '#root'

interface FolderOption {
  value: string
  title: string
  disabled: boolean
  children: FolderOption[]
}

/** 文件夹树；`allowed` 为 false 的不能选（编辑文件夹时不能选它自己和它的下级）。 */
function folderOptions(nodes: FavNode[], allowed: (id: string) => boolean): FolderOption[] {
  return nodes
    .filter((n) => n.fav.kind === 'folder')
    .map((n) => ({
      value: n.fav.id,
      title: n.fav.name,
      disabled: !allowed(n.fav.id),
      children: folderOptions(n.children, allowed),
    }))
}

export default function FavoriteDialog() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message } = App.useApp()
  const { draft, close } = useFavoriteDialog()
  const [form] = Form.useForm<Values>()
  const { data: favs = [] } = useQuery({ queryKey: ['favorites'], queryFn: () => api.favorites(), enabled: !!draft })
  const editing = draft?.mode === 'edit' ? draft.fav : null
  const folders = useMemo(() => {
    const allowed = (id: string) => !editing || canMoveInto(favs, editing.id, id)
    return [{ value: ROOT, title: t('app.favorites.root'), disabled: false, children: folderOptions(buildTree(favs), allowed) }]
  }, [favs, t, editing])

  const isItem = draft !== null && (draft.mode === 'edit' ? draft.fav.kind === 'item' : draft.kind === 'item')

  useEffect(() => {
    if (!draft) return
    const last = useSettings.getState().values[LAST_FOLDER_KEY]
    const lastFolder = last && favs.some((f) => f.id === last && f.kind === 'folder') ? last : ROOT
    if (draft.mode === 'edit') {
      form.setFieldsValue({ name: draft.fav.name, folder: draft.fav.parent_id ?? ROOT, sql: draft.fav.sql ?? undefined, note: draft.fav.note ?? '' })
    } else if (draft.kind === 'item') {
      form.setFieldsValue({ name: defaultName(draft.sql), folder: lastFolder, sql: draft.sql, note: '' })
    } else {
      form.setFieldsValue({ name: '', folder: draft.parentId ?? ROOT })
    }
    // 只在打开时填一次（不依赖 favs）：文件夹列表晚到时不覆盖用户已改的内容
  }, [draft, form])

  const submit = async () => {
    if (!draft) return
    const v = await form.validateFields()
    try {
      let saved: Favorite
      if (draft.mode === 'edit') {
        const parent = v.folder === ROOT ? null : v.folder
        saved = await api.updateFavorite(draft.fav.id, {
          name: v.name,
          ...(isItem ? { sql: v.sql, note: v.note ?? '' } : {}),
          // 换了文件夹：放在新文件夹的末尾（服务端把超出的位置当作末尾）
          ...(parent !== draft.fav.parent_id ? { move: { parent_id: parent, position: Number.MAX_SAFE_INTEGER } } : {}),
        })
      } else {
        const parent = v.folder === ROOT ? null : v.folder
        saved = await api.createFavorite(
          draft.kind === 'item'
            ? { parent_id: parent, kind: 'item', name: v.name, sql: v.sql, note: v.note || undefined, driver: draft.driver, variant: draft.variant }
            : { parent_id: parent, kind: 'folder', name: v.name },
        )
        if (draft.kind === 'item') useSettings.getState().set(LAST_FOLDER_KEY, parent)
      }
      await qc.invalidateQueries({ queryKey: ['favorites'] })
      if (draft.mode === 'create' && draft.kind === 'item') message.success(t('app.favorites.saved', { name: saved.name }))
      close()
    } catch (e) {
      message.error((e as Error).message)
    }
  }

  const title = !draft
    ? ''
    : draft.mode === 'edit'
      ? t(isItem ? 'app.favorites.editTitle' : 'app.favorites.renameTitle')
      : t(isItem ? 'app.favorites.addTitle' : 'app.favorites.newFolderTitle')

  return (
    <Modal title={title} open={!!draft} onOk={() => void submit()} onCancel={close} destroyOnHidden width={isItem ? 560 : 400}>
      <Form form={form} layout="vertical" style={{ marginTop: 12 }}>
        <Form.Item name="name" label={t('app.favorites.name')} rules={[{ required: true, whitespace: true, message: t('app.favorites.nameRequired') }]}>
          <Input autoFocus maxLength={200} />
        </Form.Item>
        <Form.Item name="folder" label={t('app.favorites.folder')}>
          <TreeSelect treeData={folders} treeDefaultExpandAll />
        </Form.Item>
        {isItem && (
          <>
            <Form.Item name="sql" label="SQL" rules={[{ required: true, whitespace: true, message: t('app.favorites.sqlRequired') }]}>
              <Input.TextArea autoSize={{ minRows: 3, maxRows: 12 }} className="mono" />
            </Form.Item>
            <Form.Item name="note" label={t('app.favorites.note')}>
              <Input.TextArea autoSize={{ minRows: 1, maxRows: 4 }} />
            </Form.Item>
          </>
        )}
      </Form>
    </Modal>
  )
}
```

- [ ] **Step 13: 新建 `web/src/features/favorites/FavoritesPanel.tsx`**

```tsx
// 收藏侧栏（M1c1 设计文档第 6 节）：多级文件夹，可拖动整理；双击插入到当前编辑器；
// 右键在新标签中打开、编辑（含换文件夹）、新建子文件夹、删除（文件夹删除前确认）。

import { App, Button, Dropdown, Tooltip, Tree } from 'antd'
import type { DataNode, TreeProps } from 'antd/es/tree'
import { IconFileCode, IconFolder, IconFolderPlus } from '@tabler/icons-react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { useApi } from '../../app/context'
import DbIcon from '../../shared/DbIcon'
import type { Favorite } from '../../shared/api'
import { useQueryActions } from '../editor/actions'
import { useFavoriteDialog } from './dialog'
import { type FavNode, buildTree, dropTarget } from './tree'

export default function FavoritesPanel() {
  const api = useApi()
  const qc = useQueryClient()
  const { t } = useTranslation()
  const { message, modal } = App.useApp()
  const { insertSql, openSql } = useQueryActions()
  const open = useFavoriteDialog((s) => s.open)
  const query = useQuery({ queryKey: ['favorites'], queryFn: () => api.favorites() })
  const favs = useMemo(() => query.data ?? [], [query.data])

  const guard = async (p: Promise<unknown>) => {
    try {
      await p
    } catch (e) {
      message.error((e as Error).message)
    }
    await qc.invalidateQueries({ queryKey: ['favorites'] })
  }

  const remove = (f: Favorite) => {
    if (f.kind === 'item') return void guard(api.deleteFavorite(f.id))
    modal.confirm({
      title: t('app.favorites.deleteFolderConfirm', { name: f.name }),
      okButtonProps: { danger: true },
      onOk: () => guard(api.deleteFavorite(f.id)),
    })
  }

  const menu = (f: Favorite) => ({
    items:
      f.kind === 'item'
        ? [
            { key: 'open', label: t('app.favorites.openInTab') },
            { key: 'edit', label: t('app.favorites.edit') },
            { type: 'divider' as const },
            { key: 'delete', label: t('app.favorites.delete'), danger: true },
          ]
        : [
            { key: 'new-folder', label: t('app.favorites.newSubfolder') },
            { key: 'edit', label: t('app.favorites.edit') },
            { type: 'divider' as const },
            { key: 'delete', label: t('app.favorites.delete'), danger: true },
          ],
    onClick: ({ key }: { key: string }) => {
      if (key === 'open' && f.sql) openSql(f.sql)
      else if (key === 'edit') open({ mode: 'edit', fav: f })
      else if (key === 'new-folder') open({ mode: 'create', kind: 'folder', parentId: f.id })
      else if (key === 'delete') remove(f)
    },
  })

  const treeData = useMemo(() => {
    const toData = (nodes: FavNode[]): DataNode[] =>
      nodes.map(({ fav, children }) => ({
        key: fav.id,
        isLeaf: fav.kind === 'item',
        icon:
          fav.kind === 'folder' ? (
            <IconFolder size={14} />
          ) : fav.driver ? (
            <DbIcon driver={fav.driver} variant={fav.variant} />
          ) : (
            <IconFileCode size={14} />
          ),
        title: fav.name,
        children: fav.kind === 'folder' ? toData(children) : undefined,
      }))
    return toData(buildTree(favs))
  }, [favs])

  const onDrop: TreeProps['onDrop'] = (info) => {
    const pos = Number(info.node.pos.split('-').pop())
    const where = !info.dropToGap ? 0 : info.dropPosition - pos < 0 ? -1 : 1
    const move = dropTarget(favs, String(info.dragNode.key), String(info.node.key), where)
    if (move) void guard(api.updateFavorite(String(info.dragNode.key), { move }))
  }

  const byId = useMemo(() => new Map(favs.map((f) => [f.id, f])), [favs])

  return (
    <div className="fav">
      <div className="row fav-bar">
        <span className="grow muted">{t('app.favorites.hint')}</span>
        <Tooltip title={t('app.favorites.newFolder')}>
          <Button type="text" size="small" icon={<IconFolderPlus size={16} />} onClick={() => open({ mode: 'create', kind: 'folder', parentId: null })} />
        </Tooltip>
      </div>
      {query.error ? (
        <div className="hist-empty">
          <div className="err">{(query.error as Error).message}</div>
          <Button size="small" onClick={() => void query.refetch()}>
            {t('app.history.retry')}
          </Button>
        </div>
      ) : !query.isPending && favs.length === 0 ? (
        <div className="hist-empty muted">{t('app.favorites.empty')}</div>
      ) : (
        <Tree
          className="fav-tree"
          showIcon
          blockNode
          draggable={{ icon: false }}
          defaultExpandAll
          treeData={treeData}
          onDrop={onDrop}
          titleRender={(node) => {
            const f = byId.get(String(node.key))
            if (!f) return null
            const label = (
              <span
                className="fav-title"
                onDoubleClick={() => {
                  if (f.sql) insertSql(f.sql)
                }}
              >
                {f.name}
              </span>
            )
            return (
              <Dropdown trigger={['contextMenu']} menu={menu(f)}>
                {f.kind === 'item' ? (
                  <Tooltip
                    placement="right"
                    mouseEnterDelay={0.6}
                    styles={{ root: { maxWidth: 560 } }}
                    title={
                      <>
                        {f.note && <div style={{ marginBottom: 4 }}>{f.note}</div>}
                        <pre className="hist-preview">{f.sql}</pre>
                      </>
                    }
                  >
                    {label}
                  </Tooltip>
                ) : (
                  label
                )}
              </Dropdown>
            )
          }}
        />
      )}
    </div>
  )
}
```

- [ ] **Step 14: 新建 `web/src/features/favorites/dialog.ts`**

```ts
// 收藏对话框的状态：编辑器（Ctrl+D、工具栏星标）、历史侧栏、收藏侧栏都可以打开它。

import { create } from 'zustand'

import type { DriverId, Favorite } from '../../shared/api'

export type FavoriteDraft =
  /** 收藏一段 SQL。 */
  | { mode: 'create'; kind: 'item'; sql: string; driver?: DriverId; variant?: string | null }
  /** 新建文件夹。 */
  | { mode: 'create'; kind: 'folder'; parentId: string | null }
  /** 编辑收藏或重命名文件夹。 */
  | { mode: 'edit'; fav: Favorite }

/** 设置：上次收藏时选的文件夹，下次默认选它。 */
export const LAST_FOLDER_KEY = 'ui.favorites.last-folder'

interface DialogState {
  draft: FavoriteDraft | null
  open(draft: FavoriteDraft): void
  close(): void
}

export const useFavoriteDialog = create<DialogState>()((set) => ({
  draft: null,
  open: (draft) => set({ draft }),
  close: () => set({ draft: null }),
}))
```

- [ ] **Step 15: 新建 `web/src/features/favorites/tree.ts`**

```ts
// 收藏的纯逻辑（M1c1 设计文档第 6 节）：平铺列表组成树、移动是否合法、拖放后的目标位置、默认名称。

import type { Favorite } from '../../shared/api'

export interface FavNode {
  fav: Favorite
  children: FavNode[]
}

/** 平铺列表 → 树。同一文件夹内按 position 排序；上级不存在的当作根。 */
export function buildTree(favs: Favorite[]): FavNode[] {
  const nodes = new Map(favs.map((f) => [f.id, { fav: f, children: [] as FavNode[] }]))
  const roots: FavNode[] = []
  for (const n of nodes.values()) {
    const parent = n.fav.parent_id ? nodes.get(n.fav.parent_id) : undefined
    ;(parent ? parent.children : roots).push(n)
  }
  const sort = (list: FavNode[]) => {
    list.sort((a, b) => a.fav.position - b.fav.position || a.fav.created_at - b.fav.created_at)
    for (const n of list) sort(n.children)
  }
  sort(roots)
  return roots
}

/** 能否把 `id` 移到文件夹 `parent`（null 为根）中：目标必须是文件夹，且不是它自己或它的下级。 */
export function canMoveInto(favs: Favorite[], id: string, parent: string | null): boolean {
  const byId = new Map(favs.map((f) => [f.id, f]))
  if (parent !== null && byId.get(parent)?.kind !== 'folder') return false
  for (let p: string | null = parent; p !== null; p = byId.get(p)?.parent_id ?? null) {
    if (p === id) return false
  }
  return true
}

/**
 * 拖放的目标：拖到 `target` 之前（-1）、之后（1）或里面（0，只对文件夹）。
 * 返回移动请求（位置是在去掉被拖动项后的同级中的下标）；不合法时为 null。
 */
export function dropTarget(
  favs: Favorite[],
  dragId: string,
  targetId: string,
  where: -1 | 0 | 1,
): { parent_id: string | null; position: number } | null {
  const target = favs.find((f) => f.id === targetId)
  if (!target || dragId === targetId) return null
  if (where === 0) {
    if (target.kind !== 'folder' || !canMoveInto(favs, dragId, target.id)) return null
    return { parent_id: target.id, position: 0 }
  }
  const parent = target.parent_id
  if (!canMoveInto(favs, dragId, parent)) return null
  const siblings = favs
    .filter((f) => f.parent_id === parent && f.id !== dragId)
    .sort((a, b) => a.position - b.position)
  const index = siblings.findIndex((f) => f.id === targetId)
  return { parent_id: parent, position: index + (where === 1 ? 1 : 0) }
}

/** 收藏的默认名称：SQL 开头的内容，换行和连续空白合成一个空格，最长 60 个字符。 */
export function defaultName(sql: string): string {
  const text = sql.trim().replace(/\s+/g, ' ')
  return text.length > 60 ? `${text.slice(0, 59)}…` : text
}

/** 文件夹的完整路径（根到它），用于文件夹选择框中显示。 */
export function folderPath(favs: Favorite[], id: string): string[] {
  const byId = new Map(favs.map((f) => [f.id, f]))
  const out: string[] = []
  for (let f = byId.get(id); f; f = f.parent_id ? byId.get(f.parent_id) : undefined) out.unshift(f.name)
  return out
}
```

- [ ] **Step 16: 修改 `web/src/features/history/HistoryPanel.tsx`**

```diff
--- a/web/src/features/history/HistoryPanel.tsx
+++ b/web/src/features/history/HistoryPanel.tsx
@@ -1,5 +1,5 @@
 // 历史侧栏（M1c1 设计文档第 6 节）：搜索、按连接和失败过滤、按日期分组、滚动加载。
-// 双击插入到当前编辑器；右键在新标签中打开、复制、删除。历史由服务端在执行时写入，执行结束后自动刷新。
+// 双击插入到当前编辑器；右键在新标签中打开、复制、加入收藏、删除。历史由服务端在执行时写入，执行结束后自动刷新。
 
 import { App, Button, Dropdown, Input, Select, Switch, Tooltip } from 'antd'
 import { IconSearch } from '@tabler/icons-react'
@@ -11,6 +11,7 @@ import { useApi } from '../../app/context'
 import DbIcon from '../../shared/DbIcon'
 import type { HistoryItem } from '../../shared/api'
 import { useQueryActions } from '../editor/actions'
+import { useFavoriteDialog } from '../favorites/dialog'
 import { PAGE, detail, firstLine, groupByDay } from './history'
 
 const ENV_COLORS: Record<string, string> = {
@@ -65,12 +66,16 @@ export default function HistoryPanel() {
     items: [
       { key: 'open', label: t('app.history.openInTab') },
       { key: 'copy', label: t('app.history.copy') },
+      { key: 'favorite', label: t('app.history.addFavorite') },
       { type: 'divider' as const },
       { key: 'delete', label: t('app.history.delete'), danger: true },
     ],
     onClick: ({ key }: { key: string }) => {
       if (key === 'open') openSql(item.sql, item.profile_id)
       else if (key === 'copy') void navigator.clipboard.writeText(item.sql)
+      else if (key === 'favorite') {
+        useFavoriteDialog.getState().open({ mode: 'create', kind: 'item', sql: item.sql, driver: item.driver, variant: item.variant })
+      }
       else if (key === 'delete') void remove(item)
     },
   })
```

- [ ] **Step 17: 修改 `web/src/features/sql/client.ts`**

```diff
--- a/web/src/features/sql/client.ts
+++ b/web/src/features/sql/client.ts
@@ -4,7 +4,7 @@
 // - worker 出错或某个方言加载失败后，该方言在本次运行中不再使用解析器（调用方退回轻量补全、不检查语法）。
 
 import type { Request, RequestBody, Response } from './protocol'
-import type { Dialect, ParseIssue, ParserLocale, Suggest } from './types'
+import type { Dialect, ParseIssue, ParserLocale, StatementRange, Suggest } from './types'
 
 export const TIMEOUT_MS = 2000
 /** 加载解析器（下载 + 预热）最多等这么久。 */
@@ -44,6 +44,10 @@ export class SqlClient {
     return this.call(dialect, { op: 'suggest', dialect, sql, offset })
   }
 
+  statementAt(dialect: Dialect, sql: string, offset: number): Promise<Outcome<StatementRange | null>> {
+    return this.call(dialect, { op: 'statement', dialect, sql, offset })
+  }
+
   private async call<T>(dialect: Dialect, req: RequestBody): Promise<Outcome<T>> {
     if (!(await this.load(dialect))) return { ok: false, reason: 'unavailable' }
     const resp = await this.send(req, TIMEOUT_MS)
```

- [ ] **Step 18: 修改 `web/src/features/sql/dtsql.ts`**

```diff
--- a/web/src/features/sql/dtsql.ts
+++ b/web/src/features/sql/dtsql.ts
@@ -3,7 +3,7 @@
 
 import type { EntityContext, MySQL, PostgreSQL, Suggestions } from 'dt-sql-parser'
 
-import type { SqlParser, Suggest, TableRef, Want } from './types'
+import { type SqlParser, type Suggest, type TableRef, type Want, pickStatement } from './types'
 
 type DtParser = MySQL | PostgreSQL
 
@@ -89,5 +89,13 @@ export function adapt(p: DtParser): SqlParser {
         tables: tablesOf(quiet(() => p.getAllEntities(sql, caret))),
       }
     },
+    statementAt(sql, offset) {
+      const slices = quiet(() => p.splitSQLByStatement(sql)) ?? []
+      return pickStatement(
+        // dt-sql-parser 的语句含结束符 `;`，去掉
+        slices.map((s) => ({ start: s.startIndex, end: s.startIndex + s.text.replace(/\s*;?\s*$/, '').length })),
+        offset,
+      )
+    },
   }
 }
```

- [ ] **Step 19: 修改 `web/src/features/sql/plsql/parser.ts`**

```diff
--- a/web/src/features/sql/plsql/parser.ts
+++ b/web/src/features/sql/plsql/parser.ts
@@ -15,7 +15,7 @@ import {
   Token,
 } from 'antlr4ng'
 
-import type { ParseIssue, SqlParser, Suggest, TableRef, Want } from '../types'
+import { type ParseIssue, type SqlParser, type Suggest, type TableRef, type Want, pickStatement } from '../types'
 import { PlSqlLexer } from './gen/PlSqlLexer'
 import { PlSqlParser, type Sql_scriptContext } from './gen/PlSqlParser'
 
@@ -305,4 +305,10 @@ export const plsql: SqlParser = {
     const pos = code.findIndex((t) => t.tokenIndex >= idx)
     return { keywords, want: [...want], qualifier: qualifierBefore(code, pos < 0 ? code.length : pos), tables: scanTables(code) }
   },
+
+  statementAt(sql, offset) {
+    // 句末的空白不算在语句里
+    const ranges = split(lex(sql)).map((p) => ({ start: p.start, end: p.start + sql.slice(p.start, p.end).trimEnd().length }))
+    return pickStatement(ranges, offset)
+  },
 }
```

- [ ] **Step 20: 修改 `web/src/features/sql/protocol.ts`**

```diff
--- a/web/src/features/sql/protocol.ts
+++ b/web/src/features/sql/protocol.ts
@@ -1,15 +1,16 @@
 // 主线程与 SQL 语言 worker 之间的消息。
 
-import type { Dialect, ParseIssue, ParserLocale, Suggest } from './types'
+import type { Dialect, ParseIssue, ParserLocale, StatementRange, Suggest } from './types'
 
 export type Request =
   | { id: number; op: 'load'; dialect: Dialect }
   | { id: number; op: 'validate'; dialect: Dialect; sql: string; locale: ParserLocale }
   | { id: number; op: 'suggest'; dialect: Dialect; sql: string; offset: number }
+  | { id: number; op: 'statement'; dialect: Dialect; sql: string; offset: number }
 
 /** 去掉 id 的请求（按各个分支分别去掉）。 */
 export type RequestBody = Request extends infer R ? (R extends Request ? Omit<R, 'id'> : never) : never
 
 export type Response =
-  | { id: number; ok: true; result: ParseIssue[] | Suggest | null }
+  | { id: number; ok: true; result: ParseIssue[] | Suggest | StatementRange | null }
   | { id: number; ok: false; error: string }
```

- [ ] **Step 21: 修改 `web/src/features/sql/types.ts`**

```diff
--- a/web/src/features/sql/types.ts
+++ b/web/src/features/sql/types.ts
@@ -42,9 +42,22 @@ export interface Suggest {
   tables: TableRef[]
 }
 
+/** 一条语句在全文中的范围：[start, end) 为 UTF-16 偏移，不含结束符。 */
+export interface StatementRange {
+  start: number
+  end: number
+}
+
 /** 语言服务：每种 Dialect 一个实现，在 worker 里运行（测试中直接调用）。 */
 export interface SqlParser {
   validate(sql: string, locale: ParserLocale): ParseIssue[]
   /** offset：光标在 sql 中的 UTF-16 偏移。 */
   suggest(sql: string, offset: number): Suggest
+  /** 光标所在的语句（规则同后端 `statement_at`：取开始位置不在光标之后的最后一条，光标在第一条之前时取第一条）；没有语句时为 null。 */
+  statementAt(sql: string, offset: number): StatementRange | null
+}
+
+/** 从按位置排好的语句中选出光标所在的一条（规则见 `SqlParser.statementAt`）。 */
+export function pickStatement(ranges: StatementRange[], offset: number): StatementRange | null {
+  return [...ranges].reverse().find((r) => r.start <= offset) ?? ranges[0] ?? null
 }
```

- [ ] **Step 22: 修改 `web/src/features/sql/worker.ts`**

```diff
--- a/web/src/features/sql/worker.ts
+++ b/web/src/features/sql/worker.ts
@@ -15,6 +15,8 @@ async function handle(r: Request): Promise<Response> {
         return { id: r.id, ok: true, result: p.validate(r.sql, r.locale) }
       case 'suggest':
         return { id: r.id, ok: true, result: p.suggest(r.sql, r.offset) }
+      case 'statement':
+        return { id: r.id, ok: true, result: p.statementAt(r.sql, r.offset) }
     }
   } catch (e) {
     return { id: r.id, ok: false, error: e instanceof Error ? e.message : String(e) }
```

- [ ] **Step 23: 修改 `web/src/locales/en/app.json`**

```diff
--- a/web/src/locales/en/app.json
+++ b/web/src/locales/en/app.json
@@ -78,7 +78,8 @@
       "runAll": "Run All",
       "cancel": "Stop",
       "format": "Format",
-      "explain": "Explain Plan"
+      "explain": "Explain Plan",
+      "favorite": "Add Current SQL to Favorites"
     },
     "window": {
       "title": "Window",
@@ -110,6 +111,29 @@
     "retry": "Retry",
     "openInTab": "Open in New Tab",
     "copy": "Copy SQL",
-    "delete": "Delete"
+    "delete": "Delete",
+    "addFavorite": "Add to Favorites"
+  },
+  "favorites": {
+    "hint": "Double-click to insert, drag to organize",
+    "newFolder": "New Folder",
+    "newSubfolder": "New Subfolder",
+    "empty": "No favorites yet. Press Ctrl+D in the editor to save the current SQL",
+    "root": "Favorites (root)",
+    "name": "Name",
+    "nameRequired": "Enter a name",
+    "folder": "Folder",
+    "note": "Note",
+    "sqlRequired": "Enter the SQL",
+    "addTitle": "Add to Favorites",
+    "editTitle": "Edit Favorite",
+    "renameTitle": "Edit Folder",
+    "newFolderTitle": "New Folder",
+    "saved": "Saved to favorites: {{name}}",
+    "nothing": "There is no SQL to save",
+    "openInTab": "Open in New Tab",
+    "edit": "Edit",
+    "delete": "Delete",
+    "deleteFolderConfirm": "Delete the folder \"{{name}}\" and everything in it?"
   }
 }
```

- [ ] **Step 24: 修改 `web/src/locales/en/editor.json`**

```diff
--- a/web/src/locales/en/editor.json
+++ b/web/src/locales/en/editor.json
@@ -4,7 +4,8 @@
     "runAll": "Run All",
     "stop": "Stop",
     "format": "Format",
-    "explain": "Explain Plan"
+    "explain": "Explain Plan",
+    "favorite": "Add to Favorites"
   },
   "toolbar": {
     "runCurrent": "Run Current Statement (Ctrl+Enter)",
@@ -18,7 +19,8 @@
     "rollback": "Rollback",
     "inTransaction": "Uncommitted transaction",
     "format": "Format (Shift+Alt+F)",
-    "explain": "Explain Plan"
+    "explain": "Explain Plan",
+    "favorite": "Add to Favorites (Ctrl+D)"
   },
   "switchProfile": {
     "title": "Switch Connection",
```

- [ ] **Step 25: 修改 `web/src/locales/zh-CN/app.json`**

```diff
--- a/web/src/locales/zh-CN/app.json
+++ b/web/src/locales/zh-CN/app.json
@@ -77,7 +77,8 @@
       "runAll": "执行全部",
       "cancel": "停止",
       "format": "格式化",
-      "explain": "执行计划"
+      "explain": "执行计划",
+      "favorite": "收藏当前 SQL"
     },
     "window": {
       "title": "窗口",
@@ -108,6 +109,29 @@
     "retry": "重试",
     "openInTab": "在新标签中打开",
     "copy": "复制 SQL",
-    "delete": "删除"
+    "delete": "删除",
+    "addFavorite": "加入收藏"
+  },
+  "favorites": {
+    "hint": "双击插入，拖动整理",
+    "newFolder": "新建文件夹",
+    "newSubfolder": "新建子文件夹",
+    "empty": "还没有收藏。在编辑器中按 Ctrl+D 收藏当前 SQL",
+    "root": "收藏（根）",
+    "name": "名称",
+    "nameRequired": "请填写名称",
+    "folder": "文件夹",
+    "note": "说明",
+    "sqlRequired": "请填写 SQL",
+    "addTitle": "收藏 SQL",
+    "editTitle": "编辑收藏",
+    "renameTitle": "编辑文件夹",
+    "newFolderTitle": "新建文件夹",
+    "saved": "已收藏：{{name}}",
+    "nothing": "没有可收藏的 SQL",
+    "openInTab": "在新标签中打开",
+    "edit": "编辑",
+    "delete": "删除",
+    "deleteFolderConfirm": "删除文件夹“{{name}}”及其中的全部内容？"
   }
 }
```

- [ ] **Step 26: 修改 `web/src/locales/zh-CN/editor.json`**

```diff
--- a/web/src/locales/zh-CN/editor.json
+++ b/web/src/locales/zh-CN/editor.json
@@ -4,7 +4,8 @@
     "runAll": "执行全部",
     "stop": "停止",
     "format": "格式化",
-    "explain": "执行计划"
+    "explain": "执行计划",
+    "favorite": "收藏当前 SQL"
   },
   "toolbar": {
     "runCurrent": "执行当前语句（Ctrl+Enter）",
@@ -18,7 +19,8 @@
     "rollback": "回滚",
     "inTransaction": "有未提交的事务",
     "format": "格式化（Shift+Alt+F）",
-    "explain": "执行计划"
+    "explain": "执行计划",
+    "favorite": "收藏（Ctrl+D）"
   },
   "switchProfile": {
     "title": "切换连接",
```

- [ ] **Step 27: 检查**

在 PowerShell 中执行（bash 里 `cd web` 会触发坏掉的 fnm 钩子，`&&` 后面的命令会被跳过）：

```powershell
$env:PATH = "D:\Environments\nvm4w\nodejs;$env:PATH"; Set-Location web; pnpm typecheck; pnpm test; pnpm build
```

Expected: tsc 无输出；29 个测试文件、189 个测试通过；打包成功。

- [ ] **Step 28: 提交**

```bash
git add web/src/app/Shell.tsx web/src/app/menus.test.ts web/src/app/menus.ts web/src/app/shell.css web/src/features/editor/QueryView.tsx web/src/features/editor/SqlEditor.tsx web/src/features/editor/Toolbar.tsx web/src/features/editor/commands.ts web/src/features/favorites/FavoriteDialog.tsx web/src/features/favorites/FavoritesPanel.tsx web/src/features/favorites/dialog.ts web/src/features/favorites/tree.test.ts web/src/features/favorites/tree.ts web/src/features/history/HistoryPanel.tsx web/src/features/sql/client.ts web/src/features/sql/dtsql.test.ts web/src/features/sql/dtsql.ts web/src/features/sql/plsql/parser.test.ts web/src/features/sql/plsql/parser.ts web/src/features/sql/protocol.ts web/src/features/sql/types.ts web/src/features/sql/worker.ts web/src/locales/en/app.json web/src/locales/en/editor.json web/src/locales/zh-CN/app.json web/src/locales/zh-CN/editor.json
git commit -m "feat(web): 收藏：多级文件夹侧栏（拖动整理、插入、在新标签中打开、编辑）、Ctrl+D 与工具栏收藏当前 SQL" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: 说明文档与实测

写 M1c1 说明文档，M1 设计文档记录 M1c 的拆分和进度。然后用浏览器模式实测。若任务 1 的复查有升级，修改说明文档的相应内容。

**Files:**
- Create: `docs/notes/m1c1-history-favorites.md`
- Modify: `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`

- [ ] **Step 1: 新建 `docs/notes/m1c1-history-favorites.md`**

````markdown
# M1c1 历史与收藏

依据：设计文档 `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`。M1c 的第一份（另两份：M1c2 对象操作、M1c3 导出与运维查询）。

## 结构

```text
crates/dblens-service/src/
  store.rs        迁移到版本 3：history、favorites 两张表；Store::lock 供多步事务使用
  history.rs      历史的存储（写入并清理、分页查询、删除、清空、设置）；HistoryLog：后台线程批量写入
  favorite.rs     收藏的存储：文件夹和收藏同表，增删改、移动（防止移进自己的下级）、同级重新编号
  session.rs      每条语句执行完写一条历史（执行计划、等待确认没有执行的不记）；统计取回的行数
  service.rs      Service::with_parts 启动历史写入线程；Service::with_history 供测试指定写法
crates/dblens-api/src/routes.rs   /history、/favorites 接口；tests/library.rs
web/src/
  features/history/   历史侧栏（HistoryPanel.tsx）与纯逻辑（history.ts：日期分组、摘要）
  features/favorites/ 收藏侧栏（FavoritesPanel.tsx）、收藏对话框（FavoriteDialog.tsx、dialog.ts）、树的逻辑（tree.ts）
  features/editor/    插入通道（commands.ts 的 sendInsert / onInsert）、openSql / insertSql、Ctrl+D 与工具栏星标
  features/sql/       光标所在的语句（SqlParser.statementAt，worker 新增 statement 请求）
  app/SettingsDialog.tsx   "执行历史"一组：开关、保留条数、清空
```

## 历史

- 写入：会话的执行循环在每条语句结束（成功、失败、被策略拒绝、被取消）时调用 `HistoryLog::record`，不阻塞执行。
  - 后台线程从有界队列（1024 条）取出，每批最多 200 条，在一个事务中写入并删除超出上限的旧记录。
  - 队列满或线程退出时丢弃并记日志。
- 内容：
  - 语句原文（不含结束符）、开始时间、耗时、当前 schema（语句执行前的）、执行时的连接名、驱动、变体；
  - 行数：查询为取回的行数，增删改为影响行数。行数达到上限时前端会取消读取，这条记为"已取消"，行数是已取回的数量。
  - 错误文字写入时按默认语言生成，之后不随界面语言变化。
- 不记：执行计划；需要确认、用户在确认框中取消的语句。
- 设置（经设置接口保存，由服务端读取）：
  - `ui.history.enabled`：值为 `off` 时不写入；
  - `ui.history.limit`：保留条数，默认 10000，范围 100–1000000。
  - 设计文档写的是 `history.*`，但设置接口只允许 `ui.` 开头的键（`ui.lang` 也是服务端读取的），所以改为 `ui.history.*`。
- 搜索用 `instr(lower(sql), lower(q))`：`%`、`_` 不是通配符；只对 ASCII 字母不区分大小写。
- 前端：执行结束 300 ms 后刷新历史列表（等后台写完）。

## 收藏

- 文件夹和收藏放在一张表里，`parent_id` 指向上级文件夹，删除文件夹时级联删除。
- 移动：插入到目标文件夹的第 `position` 个位置，超出时放在末尾；原文件夹和目标文件夹都重新编号，不留空位。
- 收藏 SQL：
  - 选中了内容就收藏选中的部分，否则收藏光标所在的语句（worker 按方言分句；解析器不可用时收藏全文）。
  - 名称默认取 SQL 开头，换行和空白合成一个空格，最长 60 个字符。设计文档写的是"取第一行"，实测中第一行常常只有 `SELECT`，所以改了。
  - 文件夹默认是上次选的（设置 `ui.favorites.last-folder`）。
- 整理：可以拖动，也可以在编辑对话框中换文件夹（移到末尾）。后者是实测后加的：自动化测试无法触发浏览器原生的拖放，编辑对话框提供一条可验证、也便于键盘操作的路径。
- Ctrl+D 覆盖了 Monaco 默认的"选中下一个相同的词"。

## 实现中的决定

- 插入：光标所在行在光标前已有内容时，先换行再插入，不和原有的 SQL 连在一起。
- 历史的连接筛选列出全部连接（设计文档写的是"全部 / 当前标签的连接"）。
- 历史写入线程用标准库线程和同步通道，不依赖 tokio 运行时（`Service::open` 可能在运行时之外调用）。

## 验证

- 后端：fmt、clippy 通过；全部测试在 `DBLENS_LANG=en` 下对 5 个实库通过（361 个）。
  - 第一次全量运行时，dblens-api 的达梦实库测试偶发连接失败。单独运行和再跑两遍都通过，判断是多个测试程序同时连达梦造成的。
- 前端：tsc 通过；29 个测试文件、189 个测试通过；打包通过。
- 浏览器实测（MariaDB，中英文界面）：
  - 执行三条语句（第二条出错）后，历史侧栏按"今天"分组显示两条：出错的显示 MariaDB 的报错原文，成功的显示"2 行"；第三条因出错停止没有执行，不在历史中。
  - "只看失败"、双击插入正常。
  - Ctrl+D 带出光标所在的语句，默认名称和文件夹正确；收藏后侧栏出现，新建文件夹、右键菜单、在编辑对话框中换文件夹正常。
  - 英文界面下两个侧栏和设置的文字正确，单复数正确。
- 未自动验证：鼠标拖动整理（浏览器原生拖放无法由自动化触发）。拖放后的位置计算有单元测试，移动接口有接口测试。
````

- [ ] **Step 2: 修改 `docs/superpowers/specs/2026-10-25-m1-workbench-design.md`**

```diff
--- a/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
+++ b/docs/superpowers/specs/2026-10-25-m1-workbench-design.md
@@ -156,6 +156,8 @@ M1b 在写计划时（2026-10-25，用户确认）再拆成两份：
 - 2026-10-25 在 M1b2 之前插入 M1i：中英双语与数据库图标（设计 `docs/superpowers/specs/2026-10-25-m1i-i18n-icons-design.md`）。
 - M1i 已完成（详细计划 `docs/superpowers/plans/2026-10-25-m1i-i18n-icons.md`，说明 `docs/notes/m1i-i18n.md`）。
 - M1b2 已完成（设计 `docs/superpowers/specs/2026-10-25-m1b2-completion-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1b2-completion.md`，说明 `docs/notes/m1b2-completion.md`）。
+- 2026-10-26 M1c 再拆成三份（用户确认）：M1c1 历史与收藏；M1c2 对象操作（查看 DDL、生成 SQL、关联查询、"查看数据"标签）；M1c3 导出与运维查询。
+- M1c1 已完成（设计 `docs/superpowers/specs/2026-10-26-m1c1-history-favorites-design.md`，详细计划 `docs/superpowers/plans/2026-10-26-m1c1-history-favorites.md`，说明 `docs/notes/m1c1-history-favorites.md`）。
 
 右键菜单中的"查看 DDL"和"生成 SQL"依赖 M1c 的后端，M1a 先不显示这两项。
 
```

- [ ] **Step 3: 检查**

全量回归：后端对 5 个实库（各 `DBLENS_TEST_*_URL` 只用命令行内联的环境变量传入，密码不写进任何文件），前端测试和打包：

```bash
export CARGO_INCREMENTAL=0
cargo fmt --all
cargo clippy --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace --all-targets -- -D warnings
DBLENS_LANG=en cargo test --config "source.mirror.registry='sparse+https://rsproxy.cn/index/'" --workspace
```

实测（浏览器模式，用新的数据目录启动 `dblens-server --web-dir web/dist`）：
1. 建 5 个测试连接（密码只经环境变量传给建连接的脚本），确认 Oracle、达梦的客户端库目录。
2. MariaDB 标签执行三条语句（第二条出错，默认出错停止）：历史侧栏"今天"下有两条，出错的显示报错原文，成功的显示行数；第三条不在历史中。
3. "只看失败"只剩出错的那条；搜索框按 SQL 过滤；双击一条，插入到编辑器（光标前已有内容时另起一行）。
4. 光标放在多行语句中按 Ctrl+D：对话框带出这条语句，名称为 SQL 开头（换行合成空格），文件夹为"收藏（根）"；确定后收藏侧栏出现这一条。
5. 新建文件夹；右键收藏 → 编辑 → 换到该文件夹，确定后收藏移进文件夹。手动试一次拖动整理。
6. 设置切到 English：两个侧栏和设置的文字为英文，行数单复数正确；关闭历史后再执行，历史不再增加。
7. 删除测试连接（系统凭据中的密码随之删除），停掉服务。

Expected: 全部通过。

- [ ] **Step 4: 提交**

```bash
git add docs/notes/m1c1-history-favorites.md docs/superpowers/specs/2026-10-25-m1-workbench-design.md
git commit -m "docs: M1c1 说明文档；M1 设计文档记录 M1c 的拆分与进度" -m "Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
