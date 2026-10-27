//! HTTP 接口：REST（连接配置、驱动、密码库、元数据、会话管理）和会话的 WebSocket 执行协议。
//! 另提供前端页面的静态文件服务（`with_web`）。桌面模式和浏览器模式（`dblens-server`）共用。所有请求都要带访问令牌，浏览器请求还要通过 Origin 校验。

mod auth;
mod error;
mod exports;
mod lang;
mod routes;
mod web;
mod ws;

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post, put};
use dblens_service::Service;
use tokio::net::TcpListener;

pub use error::{ApiError, status_of};
pub use web::with_web;
pub use ws::{DEFAULT_MAX_ROWS, utf16_offset};

/// 接口的配置。
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// 访问令牌。
    pub token: String,
    /// 除同源外还允许的 Origin，如桌面模式的 `tauri://localhost`、前端开发服务器 `http://localhost:5173`。
    pub allowed_origins: Vec<String>,
    /// 页面和服务在同一台机器上（桌面程序）：导出可以直接写到页面给出的路径。
    /// 浏览器模式（`dblens-server`）页面可能在另一台机器上，为 false，导出一律写临时文件后下载。
    pub local_files: bool,
}

/// 生成随机访问令牌（64 个十六进制字符）。
pub fn new_token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Service>,
    pub config: Arc<ApiConfig>,
}

/// 全部接口。
pub fn router(service: Arc<Service>, config: ApiConfig) -> Router {
    let state = AppState { service, config: Arc::new(config) };
    let api = Router::new()
        .route("/drivers", get(routes::drivers))
        .route("/drivers/{driver}/lib-dir", put(routes::set_lib_dir))
        .route("/settings", get(routes::settings))
        .route("/settings/{key}", put(routes::set_setting))
        .route("/history", get(routes::history).delete(routes::clear_history))
        .route("/history/{id}", delete(routes::delete_history))
        .route("/favorites", get(routes::favorites).post(routes::create_favorite))
        .route("/favorites/{id}", put(routes::update_favorite).delete(routes::delete_favorite))
        .route("/secrets", get(routes::secret_status))
        .route("/secrets/init", post(routes::init_vault))
        .route("/secrets/unlock", post(routes::unlock))
        .route("/secrets/lock", post(routes::lock))
        .route("/profiles", get(routes::profiles).post(routes::create_profile))
        .route("/profiles/test", post(routes::test_connection))
        .route(
            "/profiles/{id}",
            get(routes::profile).put(routes::update_profile).delete(routes::delete_profile),
        )
        .route("/profiles/{id}/password", put(routes::set_password))
        .route("/profiles/{id}/refresh", post(routes::refresh))
        .route("/profiles/{id}/schemas", get(routes::schemas))
        .route("/profiles/{id}/schemas/{schema}/objects", get(routes::objects))
        .route("/profiles/{id}/schemas/{schema}/tables", get(routes::tables))
        .route("/profiles/{id}/schemas/{schema}/views", get(routes::views))
        .route("/profiles/{id}/schemas/{schema}/sequences", get(routes::sequences))
        .route("/profiles/{id}/schemas/{schema}/routines", get(routes::routines))
        .route("/profiles/{id}/schemas/{schema}/synonyms", get(routes::synonyms))
        .route("/profiles/{id}/schemas/{schema}/source", get(routes::source))
        .route("/profiles/{id}/schemas/{schema}/ddl", get(routes::ddl))
        .route("/profiles/{id}/schemas/{schema}/generate", get(routes::generate))
        .route("/profiles/{id}/schemas/{schema}/relations", get(routes::relations))
        .route("/profiles/{id}/schemas/{schema}/data-sql", post(routes::data_sql))
        .route("/profiles/{id}/ops", get(exports::ops))
        .route("/exports", post(exports::create))
        .route("/exports/{id}", get(exports::status).delete(exports::cancel_or_remove))
        .route("/exports/{id}/file", get(exports::file))
        .route("/sessions", get(routes::sessions).post(routes::open_session))
        .route("/sessions/{id}", get(routes::session).delete(routes::close_session))
        .route("/sessions/{id}/autocommit", post(routes::set_autocommit))
        .route("/sessions/{id}/schema", post(routes::set_schema))
        .route("/sessions/{id}/commit", post(routes::commit))
        .route("/sessions/{id}/rollback", post(routes::rollback))
        .route("/sessions/{id}/reconnect", post(routes::reconnect))
        .route("/sessions/{id}/cancel", post(routes::cancel))
        .route("/sessions/{id}/ws", get(ws::upgrade))
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        // 最外层：令牌错误等也按请求语言输出
        .layer(middleware::from_fn(lang::negotiate))
        .with_state(state);
    Router::new().nest("/api", api)
}

/// 在已绑定的端口上提供服务，直到出错（桌面模式绑定 `127.0.0.1:0` 的随机端口后调用）。
pub async fn serve(listener: TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(listener, app).await
}
