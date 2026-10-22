//! 用内存假驱动验证阻塞适配器：专用线程、分批取数、错误传递、取消、提前丢弃执行流、事务命令转发。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dblens_core::{Error, Result, Value};
use dblens_driver::blocking::{BlockingConnection, BlockingExec, into_async};
use dblens_driver::{CancelHandle, ColumnMeta, ExecEvent, ServerInfo, collect_all};

struct Fake {
    log: Arc<Mutex<Vec<String>>>,
    cancel_flag: Arc<AtomicBool>,
}

struct FakeExec {
    rows_left: Vec<Vec<Value>>,
    started: bool,
    ended: bool,
}

impl BlockingExec for FakeExec {
    fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if !self.started {
            self.started = true;
            return Ok(Some(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "INT")] }));
        }
        if !self.rows_left.is_empty() {
            let take = max_rows.min(self.rows_left.len());
            return Ok(Some(ExecEvent::Rows(self.rows_left.drain(..take).collect())));
        }
        if !self.ended {
            self.ended = true;
            return Ok(Some(ExecEvent::ResultEnd));
        }
        Ok(None)
    }
}

impl BlockingConnection for Fake {
    fn execute<'a>(&'a mut self, sql: &'a str, _params: &'a [Value]) -> Result<Box<dyn BlockingExec + 'a>> {
        let thread = std::thread::current().name().unwrap_or("").to_string();
        self.log.lock().unwrap().push(format!("{thread}:{sql}"));
        if sql == "fail" {
            return Err(Error::Sql { code: "1".into(), message: "坏语句".into(), position: None });
        }
        if sql == "sleep" {
            let started = Instant::now();
            while !self.cancel_flag.swap(false, Ordering::SeqCst) {
                assert!(started.elapsed() < Duration::from_secs(5), "取消没有到达");
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(Error::Cancelled);
        }
        let n: i64 = sql.strip_prefix("rows:").expect("测试 SQL 格式为 rows:N").parse().unwrap();
        Ok(Box::new(FakeExec {
            rows_left: (1..=n).map(|i| vec![Value::Int(i)]).collect(),
            started: false,
            ended: false,
        }))
    }
    fn set_autocommit(&mut self, on: bool) -> Result<()> {
        self.log.lock().unwrap().push(format!("autocommit={on}"));
        Ok(())
    }
    fn commit(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("commit".into());
        Ok(())
    }
    fn rollback(&mut self) -> Result<()> {
        self.log.lock().unwrap().push("rollback".into());
        Ok(())
    }
    fn ping(&mut self) -> Result<()> {
        Ok(())
    }
    fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "Fake".into(), version: "1".into() })
    }
    fn close(self: Box<Self>) -> Result<()> {
        self.log.lock().unwrap().push("close".into());
        Ok(())
    }
}

fn setup() -> (Box<dyn dblens_driver::Connection>, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let flag = Arc::new(AtomicBool::new(false));
    let cancel = {
        let flag = flag.clone();
        CancelHandle::new(move || {
            let flag = flag.clone();
            async move {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
        })
    };
    let conn =
        into_async(Fake { log: log.clone(), cancel_flag: flag }, cancel, "dblens-test-worker").unwrap();
    (conn, log)
}

#[tokio::test]
async fn streams_rows_in_batches_on_worker_thread() {
    let (mut conn, log) = setup();
    let mut stream = conn.execute("rows:5", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 2).await.unwrap();
    let sizes: Vec<usize> =
        events.iter().filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.len()) } else { None }).collect();
    assert_eq!(sizes, vec![2, 2, 1]);
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd));
    assert_eq!(log.lock().unwrap()[0], "dblens-test-worker:rows:5");
}

#[tokio::test]
async fn execute_error_propagates_and_connection_survives() {
    let (mut conn, _) = setup();
    let err = conn.execute("fail", &[]).await.err().expect("应返回错误");
    assert!(matches!(err, Error::Sql { .. }), "{err:?}");
    conn.ping().await.unwrap();
    let mut stream = conn.execute("rows:1", &[]).await.unwrap();
    assert_eq!(collect_all(stream.as_mut(), 10).await.unwrap().len(), 3);
}

#[tokio::test]
async fn cancel_interrupts_running_statement() {
    let (mut conn, _) = setup();
    let handle = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.cancel().await
    });
    let started = Instant::now();
    let err = conn.execute("sleep", &[]).await.err().expect("应被取消");
    canceller.await.unwrap().unwrap();
    assert_eq!(err, Error::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
    conn.ping().await.unwrap();
}

#[tokio::test]
async fn dropping_stream_early_allows_next_execute() {
    let (mut conn, _) = setup();
    {
        let mut stream = conn.execute("rows:10", &[]).await.unwrap();
        assert!(matches!(stream.next(3).await.unwrap(), Some(ExecEvent::ResultStart { .. })));
    }
    let mut stream = conn.execute("rows:3", &[]).await.unwrap();
    let rows: usize = collect_all(stream.as_mut(), 100)
        .await
        .unwrap()
        .iter()
        .map(|e| if let ExecEvent::Rows(r) = e { r.len() } else { 0 })
        .sum();
    assert_eq!(rows, 3);
}

#[tokio::test]
async fn transaction_calls_are_forwarded() {
    let (mut conn, log) = setup();
    conn.set_autocommit(false).await.unwrap();
    conn.commit().await.unwrap();
    conn.rollback().await.unwrap();
    assert_eq!(conn.server_info().await.unwrap().product, "Fake");
    conn.close().await.unwrap();
    assert_eq!(*log.lock().unwrap(), vec!["autocommit=false", "commit", "rollback", "close"]);
}
