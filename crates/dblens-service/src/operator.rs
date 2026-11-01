//! 操作者（审计版设计 3.1 节）：谁在执行。单机版固定为本机用户；审计版由鉴权层用 `Operator::scope` 设定。
//! 用 task-local 传递，后台任务要在 `tokio::spawn` 之前取出并自己保存。

use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Operator {
    pub id: String,
    pub name: String,
}

tokio::task_local! {
    static CURRENT: Operator;
}

impl Operator {
    pub const LOCAL_ID: &'static str = "local";

    pub fn local() -> Self {
        Self { id: Self::LOCAL_ID.into(), name: String::new() }
    }

    /// 当前任务的操作者，没有设定时是本机用户。
    pub fn current() -> Self {
        CURRENT.try_with(Clone::clone).unwrap_or_else(|_| Self::local())
    }

    pub async fn scope<F: Future>(op: Operator, fut: F) -> F::Output {
        CURRENT.scope(op, fut).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_defaults_to_local_and_follows_scope() {
        assert_eq!(Operator::current().id, Operator::LOCAL_ID);
        let op = Operator { id: "u1".into(), name: "张三".into() };
        let seen = Operator::scope(op.clone(), async { Operator::current() }).await;
        assert_eq!(seen, op);
        assert_eq!(Operator::current().id, Operator::LOCAL_ID);
    }
}
