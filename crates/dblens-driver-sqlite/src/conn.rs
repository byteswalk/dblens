//! SQLite 连接（在专用线程上运行的同步实现）与执行流。

use std::collections::VecDeque;

use dblens_core::{Result, Row, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{ColumnMeta, ExecEvent, ServerInfo};
use rusqlite::{Rows, Statement};

use crate::error::map_err;
use crate::value::{sqlite_value, to_sql};

pub(crate) struct SqliteConnection {
    conn: rusqlite::Connection,
    /// 手动提交模式：执行语句前若不在事务中，先 `BEGIN`。
    autocommit: bool,
    lob_limit: usize,
}

impl SqliteConnection {
    pub(crate) fn new(conn: rusqlite::Connection) -> Self {
        SqliteConnection { conn, autocommit: true, lob_limit: dblens_core::DEFAULT_LOB_PREVIEW_BYTES }
    }

    fn in_transaction(&self) -> bool {
        !self.conn.is_autocommit()
    }
}

/// 执行流。字段的声明顺序就是释放顺序：行游标必须先于它借用的语句释放。
pub(crate) struct SqliteExec<'a> {
    pending: VecDeque<ExecEvent>,
    /// 借用 `stmt` 的行游标。生命周期写成 'static 只是为了能和语句放在同一个结构里，见 `execute`。
    rows: Option<Rows<'static>>,
    stmt: Option<Box<Statement<'a>>>,
    lob_limit: usize,
}

impl BlockingExec for SqliteExec<'_> {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let Some(rs) = self.rows.as_mut() else {
            return Ok(None);
        };
        let mut rows: Vec<Row> = Vec::new();
        let mut ended = false;
        while rows.len() < max_rows.max(1) {
            match rs.next().map_err(map_err)? {
                Some(row) => {
                    let n = row.as_ref().column_count();
                    let mut values = Vec::with_capacity(n);
                    for i in 0..n {
                        values.push(sqlite_value(row.get_ref(i).map_err(map_err)?, self.lob_limit));
                    }
                    rows.push(values);
                }
                None => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.rows = None;
            self.stmt = None;
            if rows.is_empty() {
                return Ok(Some(ExecEvent::ResultEnd));
            }
            self.pending.push_back(ExecEvent::ResultEnd);
        }
        Ok(Some(ExecEvent::Rows(rows)))
    }
}

impl BlockingConnection for SqliteConnection {
    fn set_lob_limit(&mut self, bytes: usize) {
        self.lob_limit = bytes;
    }

    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        if !self.autocommit && !self.in_transaction() {
            self.conn.execute_batch("BEGIN").map_err(map_err)?;
        }
        // SQLite 按字节报告出错位置，换成字符偏移
        let sql_err = |e| map_err(e).position_bytes_to_chars(sql);
        let mut stmt = Box::new(self.conn.prepare(sql).map_err(sql_err)?);
        for (i, p) in params.iter().enumerate() {
            stmt.raw_bind_parameter(i + 1, to_sql(p)?).map_err(map_err)?;
        }
        let lob_limit = self.lob_limit;
        if stmt.column_count() == 0 {
            let before = self.conn.total_changes();
            stmt.raw_execute().map_err(sql_err)?;
            // changes() 只在 INSERT、UPDATE、DELETE 后更新：DDL 等语句没有改动行时报 0，而不是上一条 DML 的行数
            let affected = if self.conn.total_changes() != before { self.conn.changes() } else { 0 };
            return Ok(Box::new(SqliteExec {
                pending: VecDeque::from([ExecEvent::Affected(affected)]),
                rows: None,
                stmt: None,
                lob_limit,
            }));
        }
        let columns: Vec<ColumnMeta> =
            stmt.columns().map(|c| ColumnMeta::new(c.name(), c.decl_type().unwrap_or(""))).collect();
        let rows = stmt.raw_query();
        // SAFETY: rows 借用的语句在堆上（Box），地址不随 SqliteExec 移动；语句放在 SqliteExec 里，
        // 且 rows 字段声明在 stmt 之前，先于语句释放（结束时也是先清 rows 再清 stmt）。
        // 语句本身借用连接 'a，执行流同样受 'a 约束，不会比连接活得久。
        let rows: Rows<'static> = unsafe { std::mem::transmute::<Rows<'_>, Rows<'static>>(rows) };
        Ok(Box::new(SqliteExec {
            pending: VecDeque::from([ExecEvent::ResultStart { columns }]),
            rows: Some(rows),
            stmt: Some(stmt),
            lob_limit,
        }))
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        // 从手动切回自动时提交未完成的事务（与 JDBC 行为一致）
        if on && !self.autocommit && self.in_transaction() {
            self.conn.execute_batch("COMMIT").map_err(map_err)?;
        }
        self.autocommit = on;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        if self.in_transaction() {
            self.conn.execute_batch("COMMIT").map_err(map_err)?;
        }
        Ok(())
    }

    fn rollback(&mut self) -> Result<()> {
        if self.in_transaction() {
            self.conn.execute_batch("ROLLBACK").map_err(map_err)?;
        }
        Ok(())
    }

    fn ping(&mut self) -> Result<()> {
        self.conn.query_row("SELECT 1", [], |_| Ok(())).map_err(map_err)
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "SQLite".into(), version: rusqlite::version().to_string() })
    }

    fn close(self: Box<Self>) -> Result<()> {
        self.conn.close().map_err(|(_, e)| map_err(e))
    }
}
