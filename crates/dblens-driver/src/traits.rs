use async_trait::async_trait;
use dblens_core::{Result, Value};

use crate::{CancelHandle, Capabilities, ConnectParams, DriverId, ExecStream, ServerInfo};

/// 一类数据库的驱动。
#[async_trait]
pub trait Driver: Send + Sync {
    fn id(&self) -> DriverId;
    fn capabilities(&self) -> Capabilities;
    async fn connect(&self, params: &ConnectParams) -> Result<Box<dyn Connection>>;
}

/// 一个数据库连接。每次只执行一条语句（拆分多条语句是方言层的职责）；
/// 只有存储过程等少数情况会返回多个结果集。
#[async_trait]
pub trait Connection: Send {
    async fn execute<'a>(&'a mut self, sql: &'a str, params: &'a [Value])
    -> Result<Box<dyn ExecStream + 'a>>;
    /// 返回可以从其他任务调用的取消句柄。
    fn cancel_handle(&self) -> CancelHandle;
    /// 切换自动提交。从手动切回自动时，未提交的事务会被提交（与 JDBC 行为一致）。
    async fn set_autocommit(&mut self, on: bool) -> Result<()>;
    async fn commit(&mut self) -> Result<()>;
    async fn rollback(&mut self) -> Result<()>;
    async fn ping(&mut self) -> Result<()>;
    async fn server_info(&mut self) -> Result<ServerInfo>;
    async fn close(self: Box<Self>) -> Result<()>;
}
