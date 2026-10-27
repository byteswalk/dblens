//! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。

mod export;
/// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
#[cfg(any(test, feature = "testing"))]
pub mod fake;
mod favorite;
mod history;
mod metadata;
mod objects;
mod policy;
mod profile;
mod registry;
mod secret;
mod service;
mod session;
mod store;

pub use export::{
    EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
    ExportStatus, ExportWarnings, MAX_RUNNING_EXPORTS,
};
pub use favorite::{Favorite, FavoriteKind, FavoriteMove, FavoritePatch, NewFavorite};
pub use history::{
    DEFAULT_HISTORY_LIMIT, HISTORY_ENABLED_SETTING, HISTORY_LIMIT_SETTING, HistoryItem, HistoryLog,
    HistoryQuery, NewHistory,
};
pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
pub use objects::{DataOrder, GenerateKind, ObjectKind};
pub use policy::{
    Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
};
pub use profile::{Environment, Profile, ProfileInput};
pub use registry::{DriverRegistry, DriverStatus, ServerSummary};
pub use secret::{SecretBackend, SecretStatus, Secrets};
pub use service::{LANG_SETTING, Service};
pub use session::{ExecRequest, SessionEvent, SessionId, SessionInfo, SessionManager, statement_at};
pub use store::{Store, default_data_dir};
