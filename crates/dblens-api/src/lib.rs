//! HTTP 接口：REST（连接配置、驱动、密码库、元数据、会话管理）和会话的 WebSocket 执行协议。
//! 桌面模式和浏览器模式（`dblens-server`）共用。所有请求都要带访问令牌，浏览器请求还要通过 Origin 校验。

mod auth;
mod error;
mod routes;
mod ws;

use std::sync::Arc;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post, put};
use dblens_service::Service;
use tokio::net::TcpListener;

pub use error::{ApiError, status_of};
pub use ws::{DEFAULT_MAX_ROWS, utf16_offset};

/// 接口的配置。
#[derive(Debug, Clone)]
pub struct ApiConfig {
    /// 访问令牌。
    pub token: String,
    /// 除同源外还允许的 Origin，如桌面模式的 `tauri://localhost`、前端开发服务器 `http://localhost:5173`。
    pub allowed_origins: Vec<String>,
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
        .route("/sessions", get(routes::sessions).post(routes::open_session))
        .route("/sessions/{id}", get(routes::session).delete(routes::close_session))
        .route("/sessions/{id}/autocommit", post(routes::set_autocommit))
        .route("/sessions/{id}/commit", post(routes::commit))
        .route("/sessions/{id}/rollback", post(routes::rollback))
        .route("/sessions/{id}/reconnect", post(routes::reconnect))
        .route("/sessions/{id}/cancel", post(routes::cancel))
        .route("/sessions/{id}/ws", get(ws::upgrade))
        .layer(middleware::from_fn_with_state(state.clone(), auth::guard))
        .with_state(state);
    Router::new().nest("/api", api)
}

/// 在已绑定的端口上提供服务，直到出错（桌面模式绑定 `127.0.0.1:0` 的随机端口后调用）。
pub async fn serve(listener: TcpListener, app: Router) -> std::io::Result<()> {
    axum::serve(listener, app).await
}
