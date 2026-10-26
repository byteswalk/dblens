//! 达梦连接（在专用线程上运行的同步实现）与执行流。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dblens_core::{Error, Result, Row, Value, msg};
use dblens_driver::blocking::{BlockingConnection, BlockingExec};
use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo};

use crate::ffi::*;
use crate::handles::{ColDesc, Con, Stmt, succeeded};
use crate::position::with_position;
use crate::types::{DmType, PREVIEW, ParamData, bytes_value, classify, param_data, type_name, with_scale};

/// 语句被 dpi_cancel 中断时 DM 返回的错误码（「操作被取消」，见技术验证笔记）。
const CANCELLED_CODE: &str = "-6515";

/// 正在执行的语句句柄，供取消使用。
#[derive(Clone, Copy)]
struct StmtPtr(dhstmt);
// 句柄只在持有 `current` 锁时由取消线程使用，工作线程释放语句前会先清空 `current`
unsafe impl Send for StmtPtr {}

type Current = Arc<Mutex<Option<StmtPtr>>>;

/// 执行或取数出错时，判断是否应报告为"已取消"：错误码为 -6515，或者执行期间发出过取消
/// （DM 的取消要约 3 秒后才生效，期间语句可能以其他错误结束）。
fn map_cancel(cancelled: &AtomicBool, e: Error) -> Error {
    let requested = cancelled.swap(false, Ordering::SeqCst);
    match &e {
        Error::Sql { code, .. } if code == CANCELLED_CODE => Error::Cancelled,
        _ if requested => Error::Cancelled,
        _ => e,
    }
}

pub(crate) struct DmConnection {
    con: Con,
    current: Current,
    cancelled: Arc<AtomicBool>,
    /// 当前是否为自动提交；连接建立时已开启。
    autocommit: bool,
}

impl DmConnection {
    pub(crate) fn new(con: Con) -> Self {
        DmConnection {
            con,
            current: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
            autocommit: true,
        }
    }

    /// 取消句柄：加锁后对当前语句调用 dpi_cancel。持锁期间工作线程不会释放该语句。
    pub(crate) fn cancel_handle(&self) -> CancelHandle {
        let (dpi, current, cancelled) = (self.con.dpi_arc(), self.current.clone(), self.cancelled.clone());
        CancelHandle::new(move || {
            let (dpi, current, cancelled) = (dpi.clone(), current.clone(), cancelled.clone());
            async move {
                tokio::task::spawn_blocking(move || -> Result<()> {
                    let guard = current
                        .lock()
                        .map_err(|_| Error::internal(msg!("internal.lock_poisoned", lock = "dm_cancel")))?;
                    if let Some(ptr) = *guard {
                        cancelled.store(true, Ordering::SeqCst);
                        // SAFETY: ptr 来自 `current`，持锁期间工作线程不会释放该语句句柄；
                        // dpi_cancel 允许从其他线程对正在执行的语句调用（技术验证 spike_2）
                        let rt = unsafe { dpi.dpi_cancel(ptr.0) };
                        if !succeeded(rt) {
                            // 取消没有发出：清除标志，免得这次执行之后的真实错误被误报为"已取消"。
                            // 不在这里读语句句柄的诊断信息：工作线程可能正在并发使用该句柄
                            cancelled.store(false, Ordering::SeqCst);
                            return Err(Error::internal(msg!("dm.cancel_failed", code = rt)));
                        }
                    }
                    Ok(())
                })
                .await
                .map_err(|e| Error::internal(msg!("internal.task_failed", task = "cancel", error = e)))?
            }
        })
    }

    fn set_current(&self, ptr: Option<dhstmt>) {
        if let Ok(mut guard) = self.current.lock() {
            *guard = ptr.map(StmtPtr);
        }
    }

    fn query_text(&self, sql: &str) -> Result<String> {
        let stmt = Stmt::new(&self.con)?;
        stmt.exec_direct(sql)?;
        if !stmt.fetch()? {
            return Err(Error::internal(msg!("driver.no_result", sql = sql)));
        }
        let (bytes, _) = stmt.get_data(1, false, usize::MAX)?.unwrap_or_default();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// 绑定参数用到的缓冲区，必须活到语句执行完毕。
struct ParamBuffers {
    data: Vec<Vec<u8>>,
    lens: Vec<slength>,
}

fn bind_params(stmt: &Stmt<'_>, params: &[Value]) -> Result<ParamBuffers> {
    let mut bufs = ParamBuffers { data: Vec::with_capacity(params.len()), lens: vec![0; params.len()] };
    let mut kinds = Vec::with_capacity(params.len());
    for v in params {
        let (c_type, sql_type, bytes, len) = match param_data(v)? {
            ParamData::Null => {
                (DSQL_C_NCHAR as i64, DSQL_VARCHAR as i64, Vec::new(), DSQL_NULL_DATA as slength)
            }
            ParamData::Text(b) => {
                let len = b.len() as slength;
                (DSQL_C_NCHAR as i64, DSQL_VARCHAR as i64, b, len)
            }
            ParamData::Binary(b) => {
                let len = b.len() as slength;
                (DSQL_C_BINARY as i64, DSQL_VARBINARY as i64, b, len)
            }
        };
        // NULL 和空值（'' 或空二进制）也给 DPI 一个真实分配的 1 字节缓冲区，不传空 Vec 的悬空指针；
        // 实际长度由 lens 给出（0 或 DSQL_NULL_DATA）
        let bytes = if bytes.is_empty() { vec![0u8] } else { bytes };
        kinds.push((c_type, sql_type, len));
        bufs.data.push(bytes);
    }
    let ParamBuffers { data, lens } = &mut bufs;
    for (i, (c_type, sql_type, len)) in kinds.into_iter().enumerate() {
        lens[i] = len;
        // SAFETY: data[i] 与 lens[i] 都已按最终长度分配，之后不再增删元素，不会重新分配；
        // 整体移动 ParamBuffers（以及其中的 Vec）不改变堆上地址。调用方把返回的 ParamBuffers
        // 存进 DmExec，与语句句柄同生命周期，因此这些地址在 exec 返回之前（以及之后）一直有效
        unsafe { stmt.bind_param(i + 1, c_type, sql_type, &mut data[i], &mut lens[i])? };
    }
    Ok(bufs)
}

/// 结果集各列的描述与类型分类。
type Columns = Vec<(ColDesc, DmType)>;

enum State {
    Rows,
    Done,
}

pub(crate) struct DmExec<'a> {
    stmt: Stmt<'a>,
    current: &'a Current,
    cancelled: &'a AtomicBool,
    _params: ParamBuffers,
    columns: Columns,
    pending: VecDeque<ExecEvent>,
    state: State,
}

impl DmExec<'_> {
    fn read_row(&self) -> Result<Row> {
        let mut row = Vec::with_capacity(self.columns.len());
        for (i, (desc, kind)) in self.columns.iter().enumerate() {
            let value = match self.stmt.get_data(i + 1, kind.is_binary(), PREVIEW)? {
                None => Value::Null,
                Some((bytes, total)) => with_scale(bytes_value(*kind, bytes, total, PREVIEW), desc.scale),
            };
            row.push(value);
        }
        Ok(row)
    }

    fn fetch_batch(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        let mut rows = Vec::new();
        let mut ended = false;
        while rows.len() < max_rows.max(1) {
            if !self.stmt.fetch()? {
                ended = true;
                break;
            }
            rows.push(self.read_row()?);
        }
        if ended {
            self.state = State::Done;
            if rows.is_empty() {
                return Ok(Some(ExecEvent::ResultEnd));
            }
            self.pending.push_back(ExecEvent::ResultEnd);
        }
        Ok(Some(ExecEvent::Rows(rows)))
    }
}

impl BlockingExec for DmExec<'_> {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        match self.state {
            State::Done => Ok(None),
            State::Rows => self.fetch_batch(max_rows).map_err(|e| map_cancel(self.cancelled, e)),
        }
    }
}

impl Drop for DmExec<'_> {
    fn drop(&mut self) {
        // 先从"当前语句"中移除，再由字段析构释放语句句柄
        if let Ok(mut guard) = self.current.lock() {
            *guard = None;
        }
    }
}

impl BlockingConnection for DmConnection {
    fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        self.cancelled.store(false, Ordering::SeqCst);
        let stmt = Stmt::new(&self.con)?;
        self.set_current(Some(stmt.h));
        let run = || -> Result<ParamBuffers> {
            if params.is_empty() {
                stmt.exec_direct(sql)?;
                Ok(ParamBuffers { data: Vec::new(), lens: Vec::new() })
            } else {
                stmt.prepare(sql)?;
                let bufs = bind_params(&stmt, params)?;
                stmt.exec()?;
                Ok(bufs)
            }
        };
        let bufs = match run() {
            Ok(bufs) => bufs,
            Err(e) => {
                self.set_current(None);
                return Err(with_position(map_cancel(&self.cancelled, e), sql));
            }
        };
        let describe = || -> Result<(Columns, VecDeque<ExecEvent>, State)> {
            let n = stmt.num_columns()?;
            let mut pending = VecDeque::new();
            let mut columns = Vec::with_capacity(n);
            if n == 0 {
                pending.push_back(ExecEvent::Affected(stmt.row_count()?));
                return Ok((columns, pending, State::Done));
            }
            for i in 1..=n {
                let desc = stmt.describe(i)?;
                let kind = classify(desc.sql_type);
                columns.push((desc, kind));
            }
            let metas = columns
                .iter()
                .map(|(d, _)| ColumnMeta {
                    name: d.name.clone(),
                    type_name: type_name(d.sql_type),
                    nullable: Some(d.nullable),
                    // 注意：VARCHAR 的精度是字节数（技术验证笔记第 7 条）
                    precision: u32::try_from(d.precision).ok().filter(|p| *p > 0),
                    scale: Some(d.scale),
                    source_table: None,
                    source_column: None,
                })
                .collect();
            pending.push_back(ExecEvent::ResultStart { columns: metas });
            Ok((columns, pending, State::Rows))
        };
        let (columns, pending, state) = match describe() {
            Ok(v) => v,
            Err(e) => {
                // 语句句柄即将随 stmt 释放，必须先从"当前语句"中移除，否则取消会作用到已释放的句柄
                self.set_current(None);
                // exec 刚返回时到达的取消也可能让 describe 出错
                return Err(map_cancel(&self.cancelled, e));
            }
        };
        Ok(Box::new(DmExec {
            stmt,
            current: &self.current,
            cancelled: &self.cancelled,
            _params: bufs,
            columns,
            pending,
            state,
        }))
    }

    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        // 约定：从手动切回自动时提交未完成的事务。DPI 设置属性时实测也会提交，
        // 但这不是文档保证的行为，所以这里显式提交
        if on && !self.autocommit {
            self.con.commit()?;
        }
        self.con.set_autocommit(on)?;
        self.autocommit = on;
        Ok(())
    }

    fn commit(&mut self) -> Result<()> {
        self.con.commit()
    }

    fn rollback(&mut self) -> Result<()> {
        self.con.rollback()
    }

    fn ping(&mut self) -> Result<()> {
        self.query_text("SELECT 1").map(|_| ())
    }

    fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo {
            product: "DM".into(),
            version: self.query_text("SELECT TOP 1 BANNER FROM V$VERSION")?,
        })
    }

    fn close(self: Box<Self>) -> Result<()> {
        // 句柄在析构时登出并释放
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_code_maps_to_cancelled_even_without_flag() {
        let flag = AtomicBool::new(false);
        let e = Error::Sql { code: "-6515".into(), message: "操作被取消".into(), position: None };
        assert_eq!(map_cancel(&flag, e), Error::Cancelled);
    }

    #[test]
    fn any_error_after_cancel_request_is_cancelled_and_flag_resets() {
        let flag = AtomicBool::new(true);
        let e = Error::Sql { code: "-2106".into(), message: "x".into(), position: None };
        assert_eq!(map_cancel(&flag, e.clone()), Error::Cancelled);
        assert!(!flag.load(Ordering::SeqCst));
        assert_eq!(map_cancel(&flag, e.clone()), e);
    }
}
