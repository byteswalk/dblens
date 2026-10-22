//! Oracle 连接（在专用线程上运行的同步实现）与执行流。

use std::collections::VecDeque;
use std::sync::Arc;

use dblens_core::{Result, Row, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{ColumnMeta, ExecEvent, ServerInfo};
use oracle::sql_type::{OracleType, ToSql};
use oracle::{ResultSet, Row as OraRow};

use crate::error::map_err;
use crate::value::{column_meta, oracle_value, to_binds};

/// 每次网络往返取回的行数。
const FETCH_ARRAY_SIZE: u32 = 500;

pub(crate) struct OracleConnection {
    conn: Arc<oracle::Connection>,
    /// 自行维护的自动提交标志（`oracle::Connection::set_autocommit` 需要 &mut，而连接要与取消句柄共享）。
    autocommit: bool,
}

impl OracleConnection {
    pub(crate) fn new(conn: Arc<oracle::Connection>) -> Self {
        OracleConnection { conn, autocommit: true }
    }
}

/// 收取 DBMS_OUTPUT 缓冲区中的全部行。
fn drain_output(conn: &oracle::Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.statement("BEGIN DBMS_OUTPUT.GET_LINE(:line, :status); END;").build().map_err(map_err)?;
    let mut lines = Vec::new();
    loop {
        // OracleType 作为输出参数的类型声明
        let line_type = OracleType::Varchar2(32767);
        let status_type = OracleType::Number(0, 0);
        stmt.execute_named(&[("line", &line_type as &dyn ToSql), ("status", &status_type as &dyn ToSql)])
            .map_err(map_err)?;
        let status: i32 = stmt.bind_value("status").map_err(map_err)?;
        if status != 0 {
            break;
        }
        let line: Option<String> = stmt.bind_value("line").map_err(map_err)?;
        lines.push(line.unwrap_or_default());
    }
    Ok(lines)
}

pub(crate) struct OraExec {
    pending: VecDeque<ExecEvent>,
    rows: Option<ResultSet<'static, OraRow>>,
}

fn convert_row(row: &OraRow) -> Result<Row> {
    row.sql_values().iter().map(oracle_value).collect()
}

impl BlockingExec for OraExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let Some(rs) = self.rows.as_mut() else {
            return Ok(None);
        };
        let mut rows = Vec::new();
        let mut ended = false;
        while rows.len() < max_rows.max(1) {
            match rs.next() {
                Some(row) => rows.push(convert_row(&row.map_err(map_err)?)?),
                None => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.rows = None;
            if rows.is_empty() {
                return Ok(Some(ExecEvent::ResultEnd));
            }
            self.pending.push_back(ExecEvent::ResultEnd);
        }
        Ok(Some(ExecEvent::Rows(rows)))
    }
}

impl BlockingConnection for OracleConnection {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        let binds = to_binds(params)?;
        let refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        // lob_locator：LOB 列以定位符返回，只读取预览部分，而不是把整个 LOB 拉到客户端
        let mut stmt = self
            .conn
            .statement(sql)
            .fetch_array_size(FETCH_ARRAY_SIZE)
            .lob_locator()
            .build()
            .map_err(map_err)?;
        if stmt.is_query() {
            let rs = stmt.into_result_set::<OraRow>(&refs).map_err(map_err)?;
            let columns: Vec<ColumnMeta> = rs.column_info().iter().map(column_meta).collect();
            return Ok(Box::new(OraExec {
                pending: VecDeque::from([ExecEvent::ResultStart { columns }]),
                rows: Some(rs),
            }));
        }
        stmt.execute(&refs).map_err(map_err)?;
        let affected = stmt.row_count().map_err(map_err)?;
        let mut pending: VecDeque<ExecEvent> = VecDeque::new();
        if stmt.is_plsql() {
            pending.extend(drain_output(&self.conn)?.into_iter().map(ExecEvent::Message));
        }
        if self.autocommit && !stmt.is_ddl() {
            self.conn.commit().map_err(map_err)?;
        }
        pending.push_back(ExecEvent::Affected(affected));
        Ok(Box::new(OraExec { pending, rows: None }))
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        // 从手动切回自动时提交未完成的事务（与 JDBC 行为一致）
        if on && !self.autocommit {
            self.conn.commit().map_err(map_err)?;
        }
        self.autocommit = on;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        self.conn.commit().map_err(map_err)
    }

    fn rollback(&mut self) -> Result<()> {
        self.conn.rollback().map_err(map_err)
    }

    fn ping(&mut self) -> Result<()> {
        self.conn.ping().map_err(map_err)
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        let (_, banner) = self.conn.server_version().map_err(map_err)?;
        Ok(ServerInfo { product: "Oracle".into(), version: banner })
    }

    fn close(self: Box<Self>) -> Result<()> {
        self.conn.close().map_err(map_err)
    }
}
