use std::fmt;
use std::future::Future;
use std::sync::Arc;

use dblens_core::Result;
use futures::future::BoxFuture;

type CancelFn = dyn Fn() -> BoxFuture<'static, Result<()>> + Send + Sync;

/// 取消正在执行的语句。可以克隆，可以在其他任务或线程中调用。
#[derive(Clone)]
pub struct CancelHandle {
    f: Arc<CancelFn>,
}

impl CancelHandle {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self { f: Arc::new(move || Box::pin(f())) }
    }

    pub async fn cancel(&self) -> Result<()> {
        (self.f)().await
    }
}

impl fmt::Debug for CancelHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CancelHandle")
    }
}
