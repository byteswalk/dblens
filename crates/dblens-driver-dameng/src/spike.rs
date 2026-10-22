//! 达梦 DPI 技术验证。手动运行：
//! `cargo test -p dblens-driver-dameng spike -- --ignored --nocapture --test-threads=1`
//! 需要环境变量 DBLENS_TEST_DM_URL。结论写入 docs/notes/dameng-dpi-spike.md。

use std::time::{Duration, Instant};

use dblens_driver::ConnectParams;
use dblens_driver::native::NativeLib;

use crate::ffi::*;
use crate::handles::{Con, Env, Stmt, succeeded};
use crate::load;

/// 不取消时在测试库上约需 130 秒的查询。
const SLOW_SQL: &str = "SELECT COUNT(*) FROM SYSOBJECTS a, SYSOBJECTS b, SYSOBJECTS c WHERE a.ID + b.ID + c.ID > 0 AND LENGTH(a.NAME || b.NAME || c.NAME) > 0";

fn connect() -> Con {
    let url = std::env::var("DBLENS_TEST_DM_URL").expect("请设置 DBLENS_TEST_DM_URL");
    let (_, p): (_, ConnectParams) = ConnectParams::from_url(&url).unwrap();
    let dpi = load::load(&p).expect("加载 dmdpi 失败");
    let env = Env::new(dpi).unwrap();
    let mut con = Con::new(env).unwrap();
    con.set_attr_int(DSQL_ATTR_LOCAL_CODE as i64, PG_UTF8 as i64).expect("设置 UTF-8 失败");
    con.set_attr_int(DSQL_ATTR_LOGIN_PORT as i64, i64::from(p.port)).unwrap();
    con.login(&p.host, &p.user, &p.password).expect("登录失败");
    con
}

fn first_text(stmt: &Stmt<'_>) -> String {
    assert!(stmt.fetch().unwrap(), "没有返回行");
    let (bytes, _) = stmt.get_data(1, false, usize::MAX).unwrap().expect("值为 NULL");
    String::from_utf8(bytes).unwrap()
}

#[test]
#[ignore]
fn spike_1_connect_and_unicode() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    stmt.exec_direct("SELECT '数洞', 1, SYSDATE FROM DUAL").unwrap();
    let n = stmt.num_columns().unwrap();
    for i in 1..=n {
        println!("列 {i}：{:?}", stmt.describe(i).unwrap());
    }
    assert_eq!(first_text(&stmt), "数洞");
    let v = Stmt::new(&con).unwrap();
    v.exec_direct("SELECT TOP 1 BANNER FROM V$VERSION").unwrap();
    println!("服务器版本：{}", first_text(&v));
}

#[test]
#[ignore]
fn spike_2_cancel_from_another_thread() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    let (dpi, h) = (con.dpi_arc(), stmt.h as usize);
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        let rt = unsafe { dpi.dpi_cancel(h as dhstmt) };
        println!("dpi_cancel 返回 {}（成功：{}）", rt as i64, succeeded(rt));
    });
    let started = Instant::now();
    // 计划原用的 `SELECT COUNT(*) FROM SYSOBJECTS a, SYSOBJECTS b, SYSOBJECTS c` 会被优化成三个计数相乘，
    // 约 0.12 秒就返回，取消请求到达时语句已结束。加上逐行计算的条件后，不取消约需 130 秒
    let result = stmt.exec_direct(SLOW_SQL);
    let exec_elapsed = started.elapsed();
    canceller.join().unwrap();
    println!("执行结果：{result:?}，exec_direct 耗时 {exec_elapsed:?}");
    assert!(result.is_err(), "查询应被取消");
    assert!(exec_elapsed < Duration::from_secs(5), "取消耗时过长");
    // 取消后连接应仍可用
    let again = Stmt::new(&con).unwrap();
    again.exec_direct("SELECT 1").unwrap();
    assert_eq!(first_text(&again), "1");
}

#[test]
#[ignore]
fn spike_3_large_clob_preview() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    // 如果这条 SQL 在当前 DM 版本上不可用，换成任意返回 10 万个字符的 CLOB 的写法，并记入笔记
    stmt.exec_direct("SELECT RPAD(TO_CLOB('a'), 100000, 'a') FROM DUAL").unwrap();
    println!("列类型：{:?}", stmt.describe(1).unwrap());
    assert!(stmt.fetch().unwrap());
    let (bytes, total) = stmt.get_data(1, false, 64 * 1024).unwrap().unwrap();
    println!("预览读取 {} 字节，数据库报告总长 {:?}", bytes.len(), total);
    assert!(bytes.len() >= 64 * 1024);
}

#[test]
#[ignore]
fn spike_4_error_code_format() {
    let con = connect();
    let stmt = Stmt::new(&con).unwrap();
    let err = stmt.exec_direct("SELECT * FROM dblens_no_such_table").unwrap_err();
    println!("不存在的表：{err:?}");
    let bad = Stmt::new(&con).unwrap();
    println!("语法错误：{:?}", bad.exec_direct("SELEC 1").unwrap_err());
}

/// 用 `buf_size` 字节的缓冲区逐块读第 `col` 列，打印每次调用的返回值和长度，返回拼接后的字节。
fn read_chunks(stmt: &Stmt<'_>, dpi: &DmDpi, col: u16, buf_size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; buf_size];
    let mut all = Vec::new();
    for call in 1.. {
        let mut len: slength = 0;
        buf.fill(0xFF);
        let rt = unsafe {
            dpi.dpi_get_data(
                stmt.h,
                col,
                DSQL_C_NCHAR as _,
                buf.as_mut_ptr() as dpointer,
                buf.len() as _,
                &mut len,
            )
        };
        let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        if call <= 3 || rt as i64 != DSQL_SUCCESS_WITH_INFO as i64 {
            println!("  第 {call} 次 dpi_get_data：rt={rt} len={len} 本次写入字节（到结尾符）={nul}");
        }
        if rt as i64 == DSQL_NO_DATA as i64 || !succeeded(rt) {
            break;
        }
        all.extend_from_slice(&buf[..nul]);
        if rt as i64 == DSQL_SUCCESS as i64 {
            break;
        }
    }
    all
}

#[test]
#[ignore]
fn spike_3b_clob_length_units() {
    let con = connect();
    let info = Stmt::new(&con).unwrap();
    info.exec_direct("SELECT UNICODE(), SF_GET_LENGTH_IN_CHAR() FROM DUAL").unwrap();
    assert!(info.fetch().unwrap());
    let charset = String::from_utf8(info.get_data(1, false, 64).unwrap().unwrap().0).unwrap();
    let in_char = String::from_utf8(info.get_data(2, false, 64).unwrap().unwrap().0).unwrap();
    println!("UNICODE()（库字符集，0=GB18030 1=UTF-8 2=EUC-KR）：{charset}；LENGTH_IN_CHAR：{in_char}");
    // RPAD 按显示宽度计长，汉字算 2，所以这里得到 5 万个汉字（UTF-8 下 15 万字节）
    let sql = "SELECT c, DBMS_LOB.GETLENGTH(c) FROM (SELECT RPAD(TO_CLOB('数'), 100000, '数') c FROM DUAL)";
    let dpi = con.dpi();
    for buf_size in [64 * 1024, 1001] {
        let stmt = Stmt::new(&con).unwrap();
        stmt.exec_direct(sql).unwrap();
        assert!(stmt.fetch().unwrap());
        println!("缓冲区 {buf_size} 字节：");
        let all = read_chunks(&stmt, dpi, 1, buf_size);
        let text = String::from_utf8(all).expect("拼接后应为有效 UTF-8");
        println!(
            "  合计 {} 字节，{} 个字符，全部为“数”：{}",
            text.len(),
            text.chars().count(),
            text.chars().all(|c| c == '数')
        );
        let n = String::from_utf8(stmt.get_data(2, false, 64).unwrap().unwrap().0).unwrap();
        println!("  DBMS_LOB.GETLENGTH：{n}");
    }
}

#[test]
#[ignore]
fn spike_2b_cancel_latency() {
    let con = connect();
    for delay in [200u64, 1000, 2500] {
        let stmt = Stmt::new(&con).unwrap();
        let (dpi, h) = (con.dpi_arc(), stmt.h as usize);
        let started = Instant::now();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(delay));
            let t = Instant::now();
            let rt = unsafe { dpi.dpi_cancel(h as dhstmt) };
            (rt, t.elapsed())
        });
        let result = stmt.exec_direct(SLOW_SQL);
        let elapsed = started.elapsed();
        let (rt, cancel_call) = canceller.join().unwrap();
        println!(
            "{delay}ms 后取消：dpi_cancel rt={rt} 调用耗时 {cancel_call:?}；exec_direct 共 {elapsed:?}，结果 {result:?}"
        );
    }
    // PL/SQL 循环也能取消
    let stmt = Stmt::new(&con).unwrap();
    let (dpi, h) = (con.dpi_arc(), stmt.h as usize);
    let started = Instant::now();
    let c = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        unsafe { dpi.dpi_cancel(h as dhstmt) }
    });
    let r = stmt.exec_direct("DECLARE i INT := 0; BEGIN WHILE i < 300000000 LOOP i := i + 1; END LOOP; END;");
    println!(
        "500ms 后取消 PL/SQL 循环：dpi_cancel rt={}；exec_direct 共 {:?}，结果 {r:?}",
        c.join().unwrap(),
        started.elapsed()
    );
    drop(stmt);
    // 语句句柄在取消后能否复用
    let stmt = Stmt::new(&con).unwrap();
    let (dpi, h) = (con.dpi_arc(), stmt.h as usize);
    let c = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        unsafe { dpi.dpi_cancel(h as dhstmt) }
    });
    let _ = stmt.exec_direct(SLOW_SQL);
    c.join().unwrap();
    stmt.exec_direct("SELECT 2").unwrap();
    println!("取消后复用同一语句句柄：{}", first_text(&stmt));
}

#[test]
#[ignore]
fn spike_5_load_with_forward_slashes() {
    // LOAD_WITH_ALTERED_SEARCH_PATH 要求反斜杠；load.rs 会统一分隔符，这里确认用正斜杠填写的目录也能加载
    let url = std::env::var("DBLENS_TEST_DM_URL").expect("请设置 DBLENS_TEST_DM_URL");
    let (_, p) = ConnectParams::from_url(&url).unwrap();
    let dir = p.options.get("dpi_lib_dir").expect("URL 中需要 dpi_lib_dir").replace('\\', "/");
    // 直接调用 open_library：load() 按目录缓存，而 Windows 上 `E:/a` 与 `E:\a` 作为 Path 相等，会命中缓存
    let path = format!("{dir}/{}", NativeLib::DamengDpi.file_name());
    println!("加载 {path}");
    load::open_library(std::path::Path::new(&path)).expect("用正斜杠路径加载 dmdpi 失败");
}

#[test]
#[ignore]
fn spike_6_update_zero_rows() {
    // 影响 0 行的 UPDATE 按 DPI/ODBC 语义可能返回 DSQL_NO_DATA，exec_direct / exec 应视为成功
    let con = connect();
    let table = "DBLENS_SPIKE_NO_DATA";
    // 先删再建，前一次运行中途失败留下的表不影响本次
    let _ = Stmt::new(&con).unwrap().exec_direct(&format!("DROP TABLE {table}"));
    Stmt::new(&con).unwrap().exec_direct(&format!("CREATE TABLE {table} (ID INT, V VARCHAR(10))")).unwrap();
    Stmt::new(&con).unwrap().exec_direct(&format!("INSERT INTO {table} VALUES (1, 'a')")).unwrap();
    let update = format!("UPDATE {table} SET V = 'b' WHERE 1 = 0");

    // 先看 DM 实际的返回值
    let raw = Stmt::new(&con).unwrap();
    let sql = std::ffi::CString::new(update.as_str()).unwrap();
    let rt = unsafe { con.dpi().dpi_exec_direct(raw.h, sql.as_ptr() as _) };
    println!("dpi_exec_direct 原始返回值：{rt}（DSQL_NO_DATA = {DSQL_NO_DATA}）");
    let raw = Stmt::new(&con).unwrap();
    raw.prepare(&update).unwrap();
    let rt = unsafe { con.dpi().dpi_exec(raw.h) };
    println!("dpi_prepare + dpi_exec 原始返回值：{rt}");

    let direct = Stmt::new(&con).unwrap();
    direct.exec_direct(&update).expect("exec_direct 影响 0 行应成功");
    assert_eq!(direct.row_count().unwrap(), 0);
    let prepared = Stmt::new(&con).unwrap();
    prepared.prepare(&update).unwrap();
    prepared.exec().expect("prepare + exec 影响 0 行应成功");
    assert_eq!(prepared.row_count().unwrap(), 0);
    println!("exec_direct / exec 均成功，row_count = 0");

    Stmt::new(&con).unwrap().exec_direct(&format!("DROP TABLE {table}")).unwrap();
}
