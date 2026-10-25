//! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。

mod policy;
mod profile;
mod registry;
mod secret;
mod store;

pub use policy::{
    Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
};
pub use profile::{Environment, Profile, ProfileInput};
pub use registry::{DriverRegistry, DriverStatus, ServerSummary};
pub use secret::{SecretBackend, SecretStatus, Secrets};
pub use store::{Store, default_data_dir};
