//! 用内存假驱动验证接口是否可用：trait 对象、流式拉取、批大小、取消句柄。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use dblens_core::{Result, Value};
use dblens_driver::*;

struct FakeDriver;
struct FakeConn {
    cancelled: Arc<AtomicUsize>,
}
struct FakeStream {
    pending: Vec<ExecEvent>,
    rows_left: Vec<Vec<Value>>,
    started: bool,
}

#[async_trait]
impl Driver for FakeDriver {
    fn id(&self) -> DriverId {
        DriverId::Mysql
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities { cancel: true, ..Default::default() }
    }
    async fn connect(&self, _params: &ConnectParams) -> Result<Box<dyn Connection>> {
        Ok(Box::new(FakeConn { cancelled: Arc::new(AtomicUsize::new(0)) }))
    }
}

#[async_trait]
impl ExecStream for FakeStream {
    async fn next(&mut self, max_rows: usize) -> Result<Option<ExecEvent>> {
        if !self.started {
            self.started = true;
            return Ok(Some(ExecEvent::ResultStart { columns: vec![ColumnMeta::new("n", "INT")] }));
        }
        if !self.rows_left.is_empty() {
            let take = max_rows.min(self.rows_left.len());
            return Ok(Some(ExecEvent::Rows(self.rows_left.drain(..take).collect())));
        }
        Ok(self.pending.pop())
    }
}

#[async_trait]
impl Connection for FakeConn {
    async fn execute<'a>(&'a mut self, _sql: &'a str, _params: &'a [Value]) -> Result<Box<dyn ExecStream + 'a>> {
        Ok(Box::new(FakeStream {
            pending: vec![ExecEvent::ResultEnd],
            rows_left: (1..=5).map(|i| vec![Value::Int(i)]).collect(),
            started: false,
        }))
    }
    fn cancel_handle(&self) -> CancelHandle {
        let counter = self.cancelled.clone();
        CancelHandle::new(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
    }
    async fn set_autocommit(&mut self, _on: bool) -> Result<()> {
        Ok(())
    }
    async fn commit(&mut self) -> Result<()> {
        Ok(())
    }
    async fn rollback(&mut self) -> Result<()> {
        Ok(())
    }
    async fn ping(&mut self) -> Result<()> {
        Ok(())
    }
    async fn server_info(&mut self) -> Result<ServerInfo> {
        Ok(ServerInfo { product: "Fake".into(), version: "1.0".into() })
    }
    async fn close(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn trait_objects_stream_in_batches() {
    let driver: Box<dyn Driver> = Box::new(FakeDriver);
    let (_, params) = ConnectParams::from_url("mysql://u:p@localhost/db").unwrap();
    let mut conn = driver.connect(&params).await.unwrap();
    let mut stream = conn.execute("select", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 2).await.unwrap();
    let sizes: Vec<usize> = events
        .iter()
        .filter_map(|e| if let ExecEvent::Rows(r) = e { Some(r.len()) } else { None })
        .collect();
    assert_eq!(sizes, vec![2, 2, 1]);
    assert!(matches!(events.first(), Some(ExecEvent::ResultStart { .. })));
    assert_eq!(events.last(), Some(&ExecEvent::ResultEnd));
}

#[tokio::test]
async fn cancel_handle_is_cloneable_and_invokes_closure() {
    let conn = FakeConn { cancelled: Arc::new(AtomicUsize::new(0)) };
    let h1 = conn.cancel_handle();
    let h2 = h1.clone();
    h1.cancel().await.unwrap();
    h2.cancel().await.unwrap();
    assert_eq!(conn.cancelled.load(Ordering::SeqCst), 2);
}
