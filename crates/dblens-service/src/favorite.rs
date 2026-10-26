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
