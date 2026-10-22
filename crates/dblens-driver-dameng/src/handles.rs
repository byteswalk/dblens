//! DPI 句柄的 RAII 封装。头文件中的整数类型在不同版本间可能有差异，
//! 因此传参一律用 `as _` 转换；输出参数的局部变量类型以 ffi.rs 为准。

use std::ffi::CString;
use std::sync::Arc;

use dblens_core::{Error, Result};

use crate::ffi::*;

/// 文本数据每次读取的块大小。
const CHUNK: usize = 64 * 1024;

pub(crate) fn succeeded(rt: DPIRETURN) -> bool {
    let rt = rt as i64;
    rt == DSQL_SUCCESS as i64 || rt == DSQL_SUCCESS_WITH_INFO as i64
}

fn is(rt: DPIRETURN, code: i64) -> bool {
    rt as i64 == code
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::internal("字符串中不能包含 \\0 字符"))
}

/// 读取句柄上的第一条诊断记录，转换为错误。
pub(crate) fn diag(dpi: &DmDpi, handle_type: i64, handle: dhandle) -> Error {
    let mut code: sdint4 = 0;
    let mut msg = vec![0u8; 4096];
    let mut len: sdint2 = 0;
    let rt = unsafe {
        dpi.dpi_get_diag_rec(
            handle_type as _,
            handle,
            1,
            &mut code,
            msg.as_mut_ptr() as _,
            msg.len() as _,
            &mut len,
        )
    };
    if !succeeded(rt) {
        return Error::internal("调用 DPI 失败，且无法读取错误信息");
    }
    msg.truncate((len.max(0) as usize).min(msg.len()));
    let message = String::from_utf8_lossy(&msg).trim_end_matches('\0').trim().to_string();
    Error::Sql { code: code.to_string(), message, position: None }
}

pub(crate) struct Env {
    pub(crate) dpi: Arc<DmDpi>,
    h: dhenv,
}

impl Env {
    pub(crate) fn new(dpi: Arc<DmDpi>) -> Result<Self> {
        let mut h: dhenv = std::ptr::null_mut();
        let rt = unsafe { dpi.dpi_alloc_env(&mut h) };
        if !succeeded(rt) {
            return Err(Error::internal("dpi_alloc_env 失败"));
        }
        Ok(Env { dpi, h })
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        unsafe { self.dpi.dpi_free_env(self.h) };
    }
}

// 句柄只在单个工作线程上使用（取消除外，见 conn.rs），可以跨线程移交
unsafe impl Send for Env {}
// Env 由多个 Con 通过 Arc 共享；共享后只用于 dpi_alloc_con / dpi_free_env（后者在最后一个引用释放时调用），
// 与 ODBC 环境句柄一样按线程安全使用
unsafe impl Sync for Env {}

pub(crate) struct Con {
    env: Arc<Env>,
    pub(crate) h: dhcon,
    logged_in: bool,
}

unsafe impl Send for Con {}

impl Con {
    pub(crate) fn new(env: Arc<Env>) -> Result<Self> {
        let mut h: dhcon = std::ptr::null_mut();
        let rt = unsafe { env.dpi.dpi_alloc_con(env.h, &mut h) };
        if !succeeded(rt) {
            return Err(diag(&env.dpi, DSQL_HANDLE_ENV as i64, env.h as dhandle));
        }
        Ok(Con { env, h, logged_in: false })
    }

    pub(crate) fn dpi(&self) -> &DmDpi {
        &self.env.dpi
    }

    pub(crate) fn dpi_arc(&self) -> Arc<DmDpi> {
        self.env.dpi.clone()
    }

    pub(crate) fn check(&self, rt: DPIRETURN) -> Result<()> {
        if succeeded(rt) { Ok(()) } else { Err(diag(self.dpi(), DSQL_HANDLE_DBC as i64, self.h as dhandle)) }
    }

    /// 设置整数型连接属性（DPI 约定：整数值直接放在指针参数里，长度传 0）。
    pub(crate) fn set_attr_int(&self, attr: i64, value: i64) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_set_con_attr(self.h, attr as _, value as usize as dpointer, 0) };
        self.check(rt)
    }

    pub(crate) fn login(&mut self, host: &str, user: &str, password: &str) -> Result<()> {
        let (host, user, password) = (cstring(host)?, cstring(user)?, cstring(password)?);
        let rt = unsafe {
            self.dpi().dpi_login(self.h, host.as_ptr() as _, user.as_ptr() as _, password.as_ptr() as _)
        };
        self.check(rt)?;
        self.logged_in = true;
        Ok(())
    }

    pub(crate) fn set_autocommit(&self, on: bool) -> Result<()> {
        self.set_attr_int(DSQL_ATTR_AUTOCOMMIT as i64, i64::from(on))
    }

    pub(crate) fn commit(&self) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_commit(self.h) };
        self.check(rt)
    }

    pub(crate) fn rollback(&self) -> Result<()> {
        let rt = unsafe { self.dpi().dpi_rollback(self.h) };
        self.check(rt)
    }
}

impl Drop for Con {
    fn drop(&mut self) {
        unsafe {
            if self.logged_in {
                self.dpi().dpi_logout(self.h);
            }
            self.dpi().dpi_free_con(self.h);
        }
    }
}

/// 结果集列的描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ColDesc {
    pub(crate) name: String,
    pub(crate) sql_type: i32,
    pub(crate) precision: u64,
    pub(crate) scale: i32,
    pub(crate) nullable: bool,
}

pub(crate) struct Stmt<'c> {
    con: &'c Con,
    pub(crate) h: dhstmt,
}

impl<'c> Stmt<'c> {
    pub(crate) fn new(con: &'c Con) -> Result<Self> {
        let mut h: dhstmt = std::ptr::null_mut();
        let rt = unsafe { con.dpi().dpi_alloc_stmt(con.h, &mut h) };
        con.check(rt)?;
        Ok(Stmt { con, h })
    }

    pub(crate) fn check(&self, rt: DPIRETURN) -> Result<()> {
        if succeeded(rt) {
            Ok(())
        } else {
            Err(diag(self.con.dpi(), DSQL_HANDLE_STMT as i64, self.h as dhandle))
        }
    }

    /// 执行语句的返回值检查：按 DPI/ODBC 语义，影响 0 行的 UPDATE/DELETE 可能返回 DSQL_NO_DATA，这不是错误。
    fn check_exec(&self, rt: DPIRETURN) -> Result<()> {
        if is(rt, DSQL_NO_DATA as i64) { Ok(()) } else { self.check(rt) }
    }

    pub(crate) fn exec_direct(&self, sql: &str) -> Result<()> {
        let sql = cstring(sql)?;
        let rt = unsafe { self.con.dpi().dpi_exec_direct(self.h, sql.as_ptr() as _) };
        self.check_exec(rt)
    }

    pub(crate) fn prepare(&self, sql: &str) -> Result<()> {
        let sql = cstring(sql)?;
        let rt = unsafe { self.con.dpi().dpi_prepare(self.h, sql.as_ptr() as _) };
        self.check(rt)
    }

    pub(crate) fn exec(&self) -> Result<()> {
        let rt = unsafe { self.con.dpi().dpi_exec(self.h) };
        self.check_exec(rt)
    }

    /// 绑定一个输入参数（`index` 从 1 开始）。DPI 只记下 `buf` 和 `len` 的地址，到 `exec` 时才读取。
    ///
    /// # Safety
    ///
    /// `buf` 和 `len` 指向的内存必须保持有效且不被移动，直到本语句随后的 `exec` 返回
    /// （或语句被释放）为止；在此之前不能释放、重新分配或移动它们。
    pub(crate) unsafe fn bind_param(
        &self,
        index: usize,
        c_type: i64,
        sql_type: i64,
        buf: &mut [u8],
        len: &mut slength,
    ) -> Result<()> {
        let precision = buf.len().max(1);
        let rt = unsafe {
            self.con.dpi().dpi_bind_param(
                self.h,
                index as _,
                DSQL_PARAM_INPUT as _,
                c_type as _,
                sql_type as _,
                precision as _,
                0,
                buf.as_mut_ptr() as dpointer,
                buf.len() as _,
                len,
            )
        };
        self.check(rt)
    }

    pub(crate) fn num_columns(&self) -> Result<usize> {
        // DPI.h：dpi_number_columns 的输出参数是 sdint2*（不是 udint2*）
        let mut n: sdint2 = 0;
        let rt = unsafe { self.con.dpi().dpi_number_columns(self.h, &mut n) };
        self.check(rt)?;
        Ok(n.max(0) as usize)
    }

    /// 描述第 `index` 列（从 1 开始）。
    pub(crate) fn describe(&self, index: usize) -> Result<ColDesc> {
        let mut name = vec![0u8; 256];
        let mut name_len: sdint2 = 0;
        let mut sql_type: sdint2 = 0;
        let mut precision: ulength = 0;
        let mut scale: sdint2 = 0;
        let mut nullable: sdint2 = 0;
        let rt = unsafe {
            self.con.dpi().dpi_desc_column(
                self.h,
                index as _,
                name.as_mut_ptr() as _,
                name.len() as _,
                &mut name_len,
                &mut sql_type,
                &mut precision,
                &mut scale,
                &mut nullable,
            )
        };
        self.check(rt)?;
        name.truncate((name_len.max(0) as usize).min(name.len()));
        Ok(ColDesc {
            name: String::from_utf8_lossy(&name).trim_end_matches('\0').to_string(),
            sql_type: sql_type as i32,
            precision,
            scale: scale as i32,
            nullable: nullable != 0,
        })
    }

    /// 取下一行；没有更多行时返回 false。
    pub(crate) fn fetch(&self) -> Result<bool> {
        let mut rows: ulength = 0;
        let rt = unsafe { self.con.dpi().dpi_fetch(self.h, &mut rows) };
        if is(rt, DSQL_NO_DATA as i64) {
            return Ok(false);
        }
        self.check(rt)?;
        Ok(true)
    }

    /// 读取当前行第 `index` 列（从 1 开始）。返回 None 表示 NULL；
    /// 否则返回（读到的字节，数据库报告的总字节数）。读满 `limit` 字节后不再继续读。
    pub(crate) fn get_data(
        &self,
        index: usize,
        binary: bool,
        limit: usize,
    ) -> Result<Option<(Vec<u8>, Option<u64>)>> {
        let c_type = if binary { DSQL_C_BINARY as i64 } else { DSQL_C_NCHAR as i64 };
        let mut out = Vec::new();
        let mut total: Option<u64> = None;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let mut len: slength = 0;
            let rt = unsafe {
                self.con.dpi().dpi_get_data(
                    self.h,
                    index as _,
                    c_type as _,
                    buf.as_mut_ptr() as dpointer,
                    buf.len() as _,
                    &mut len,
                )
            };
            if is(rt, DSQL_NO_DATA as i64) {
                break;
            }
            self.check(rt)?;
            if len == DSQL_NULL_DATA as slength {
                return Ok(None);
            }
            // 文本以 \0 结尾，缓冲区写满时有效数据要扣掉结尾符
            let room = if binary { buf.len() } else { buf.len() - 1 };
            if total.is_none() && len >= 0 {
                total = Some(len as u64);
            }
            let got = if len < 0 || len as usize > room { room } else { len as usize };
            out.extend_from_slice(&buf[..got]);
            if is(rt, DSQL_SUCCESS as i64) || out.len() > limit {
                break;
            }
        }
        Ok(Some((out, total)))
    }

    pub(crate) fn row_count(&self) -> Result<u64> {
        let mut n: sdint8 = 0;
        let rt = unsafe { self.con.dpi().dpi_row_count(self.h, &mut n) };
        self.check(rt)?;
        Ok(n.max(0) as u64)
    }
}

impl Drop for Stmt<'_> {
    fn drop(&mut self) {
        unsafe { self.con.dpi().dpi_free_stmt(self.h) };
    }
}
