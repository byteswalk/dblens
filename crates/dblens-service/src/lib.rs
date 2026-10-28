//! 服务层：连接配置、密码、驱动注册表、会话、元数据缓存、执行策略。供 `dblens-api` 使用，自身不处理 HTTP。

mod changes;
mod compare;
mod design;
mod export;
/// 测试用的假驱动，供本 crate 和 `dblens-api` 的测试使用（feature `testing`）。
#[cfg(any(test, feature = "testing"))]
pub mod fake;
mod favorite;
mod history;
mod import;
mod metadata;
mod model;
mod objects;
mod policy;
mod profile;
mod registry;
mod secret;
mod service;
mod session;
mod sqlfile;
mod store;
mod task;

pub use changes::ChangeSet;
pub use compare::{
    CompareRequest, CompareResult, CompareSideRef, ModelWriteInfo, SyncPreview, SyncRequest, SyncStarted,
};
pub use design::{DesignPreview, DesignRequest, DesignTable, PreviewStatement, RebuildInfo};
pub use export::{
    EXPORT_LOB_LIMIT, ExportFormat, ExportManager, ExportOptions, ExportRequest, ExportSource, ExportState,
    ExportStatus, ExportWarnings, MAX_RUNNING_EXPORTS,
};
pub use favorite::{Favorite, FavoriteKind, FavoriteMove, FavoritePatch, NewFavorite};
pub use history::{
    DEFAULT_HISTORY_LIMIT, HISTORY_ENABLED_SETTING, HISTORY_LIMIT_SETTING, HistoryItem, HistoryLog,
    HistoryQuery, NewHistory,
};
pub use import::{
    ColumnMapping, DEFAULT_BATCH, EmptyAs, FileFormat, FileOptions, FilePreview, FileReader, FileRow,
    ImportRequest, NewColumn, NewTable, OnError, ReadWarnings, TextEncoding, Transaction, convert,
    infer_type, new_table, preview_file,
};
pub use metadata::{METADATA_POOL_SIZE, MetadataCache, MetadataService};
pub use model::{
    FileError, LogicalKey, ModelDatabase, ModelDesignPreview, ModelDesignRequest, ModelInfo, ModelStore, ModelSummary,
    ModelTableView, ModelWrite, NewModel, RECENT_MODELS_SETTING, ReverseRequest, SchemaRenames, SchemaTables,
    TableEntry,
};
pub use objects::{DataOrder, GenerateKind, ObjectKind};
pub use policy::{
    Decision, DefaultPolicy, ExecContext, ObjectRef, Outcome, Policy, StatementInfo, StatementKind, classify,
};
pub use profile::{Environment, Profile, ProfileInput};
pub use registry::{DriverRegistry, DriverStatus, ServerSummary};
pub use secret::{SecretBackend, SecretStatus, Secrets};
pub use service::{LANG_SETTING, Service};
pub use session::{
    ApplyFailure, ApplyOutcome, ExecRequest, SessionEvent, SessionId, SessionInfo, SessionManager,
    statement_at,
};
pub use sqlfile::SqlFileRequest;
pub use store::{Store, default_data_dir};
pub use task::{
    ConfirmItem, FileSource, MAX_FAILURES, MAX_RUNNING_TASKS, TaskConfirm, TaskFailure, TaskKind,
    TaskManager, TaskPhase, TaskProgress, TaskState, TaskStatus, UploadInfo,
};
