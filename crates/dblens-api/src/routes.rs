//! REST 接口。路径都在 `/api` 下，请求和响应都是 JSON；出错时的响应体见 `error.rs`。

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_core::Error;
use dblens_core::schema::{Routine, Sequence, Synonym, Table, View};
use dblens_dialect::meta::{MetaKey, ObjectInfo, SchemaInfo};
use dblens_driver::DriverId;
use dblens_service::{
    DriverStatus, Profile, ProfileInput, SecretStatus, ServerSummary, SessionId, SessionInfo,
};
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiResult;

// ---- 驱动 ----

pub async fn drivers(State(s): State<AppState>) -> ApiResult<Json<Vec<DriverStatus>>> {
    Ok(Json(s.service.registry.statuses()?))
}

#[derive(Deserialize)]
pub struct LibDirBody {
    /// 绝对路径；null 表示清除。
    pub path: Option<PathBuf>,
}

/// 确认（或清除）原生客户端库目录。
pub async fn set_lib_dir(
    State(s): State<AppState>,
    Path(driver): Path<DriverId>,
    Json(body): Json<LibDirBody>,
) -> ApiResult<Json<DriverStatus>> {
    s.service.registry.set_lib_dir(driver, body.path.as_deref())?;
    Ok(Json(s.service.registry.status(driver)?))
}

// ---- 密码库 ----

pub async fn secret_status(State(s): State<AppState>) -> ApiResult<Json<SecretStatus>> {
    Ok(Json(s.service.secrets.status()?))
}

#[derive(Deserialize)]
pub struct MasterBody {
    pub master: String,
}

pub async fn init_vault(
    State(s): State<AppState>,
    Json(b): Json<MasterBody>,
) -> ApiResult<Json<SecretStatus>> {
    s.service.secrets.init_vault(&b.master)?;
    Ok(Json(s.service.secrets.status()?))
}

pub async fn unlock(State(s): State<AppState>, Json(b): Json<MasterBody>) -> ApiResult<Json<SecretStatus>> {
    s.service.secrets.unlock(&b.master)?;
    Ok(Json(s.service.secrets.status()?))
}

pub async fn lock(State(s): State<AppState>) -> ApiResult<Json<SecretStatus>> {
    s.service.secrets.lock()?;
    Ok(Json(s.service.secrets.status()?))
}

// ---- 连接配置 ----

pub async fn profiles(State(s): State<AppState>) -> ApiResult<Json<Vec<Profile>>> {
    Ok(Json(s.service.profiles()?))
}

#[derive(Deserialize)]
pub struct CreateProfile {
    pub profile: ProfileInput,
    #[serde(default)]
    pub password: Option<String>,
}

pub async fn create_profile(
    State(s): State<AppState>,
    Json(b): Json<CreateProfile>,
) -> ApiResult<(StatusCode, Json<Profile>)> {
    Ok((StatusCode::CREATED, Json(s.service.create_profile(b.profile, b.password.as_deref())?)))
}

pub async fn profile(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Profile>> {
    Ok(Json(s.service.profile(&id)?))
}

pub async fn update_profile(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProfileInput>,
) -> ApiResult<Json<Profile>> {
    Ok(Json(s.service.update_profile(&id, input).await?))
}

pub async fn delete_profile(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.delete_profile(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PasswordBody {
    /// null 表示清除保存的密码。
    pub password: Option<String>,
}

pub async fn set_password(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<PasswordBody>,
) -> ApiResult<StatusCode> {
    s.service.set_password(&id, b.password.as_deref()).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct TestBody {
    pub profile: ProfileInput,
    #[serde(default)]
    pub password: String,
}

/// 测试一个（可能尚未保存的）连接配置。
pub async fn test_connection(
    State(s): State<AppState>,
    Json(b): Json<TestBody>,
) -> ApiResult<Json<ServerSummary>> {
    Ok(Json(s.service.test_connection(b.profile, &b.password).await?))
}

// ---- 元数据 ----

#[derive(Deserialize)]
pub struct FilterQuery {
    pub filter: Option<String>,
}

pub async fn schemas(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<FilterQuery>,
) -> ApiResult<Json<Vec<SchemaInfo>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.schemas(&p, q.filter.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct ObjectsQuery {
    /// tables / views / materialized_views，默认 tables。
    pub kind: Option<MetaKey>,
    pub filter: Option<String>,
}

pub async fn objects(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(q): Query<ObjectsQuery>,
) -> ApiResult<Json<Vec<ObjectInfo>>> {
    let p = s.service.profile(&id)?;
    let kind = q.kind.unwrap_or(MetaKey::Tables);
    Ok(Json(s.service.metadata.objects(&p, &schema, kind, q.filter.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct TablesQuery {
    /// 只取这一张表；不给则取整个 schema。
    pub table: Option<String>,
}

pub async fn tables(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(q): Query<TablesQuery>,
) -> ApiResult<Json<Vec<Table>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.tables(&p, &schema, q.table.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct ViewsQuery {
    /// views / materialized_views，默认 views。
    pub kind: Option<MetaKey>,
}

pub async fn views(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(q): Query<ViewsQuery>,
) -> ApiResult<Json<Vec<View>>> {
    let p = s.service.profile(&id)?;
    let kind = q.kind.unwrap_or(MetaKey::Views);
    if !matches!(kind, MetaKey::Views | MetaKey::MaterializedViews) {
        return Err(Error::invalid("kind 只能是 views 或 materialized_views").into());
    }
    Ok(Json(s.service.metadata.views(&p, &schema, kind).await?))
}

pub async fn sequences(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
) -> ApiResult<Json<Vec<Sequence>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.sequences(&p, &schema).await?))
}

pub async fn routines(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
) -> ApiResult<Json<Vec<Routine>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.routines(&p, &schema).await?))
}

pub async fn synonyms(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
) -> ApiResult<Json<Vec<Synonym>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.synonyms(&p, &schema).await?))
}

#[derive(Deserialize)]
pub struct SourceQuery {
    /// view_definition 或 routine_source。
    pub kind: MetaKey,
    pub name: String,
    /// routine_source 用：PROCEDURE / FUNCTION / PACKAGE / PACKAGE BODY。
    pub routine_kind: Option<String>,
}

pub async fn source(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(q): Query<SourceQuery>,
) -> ApiResult<Json<Option<String>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.source(&p, &schema, q.kind, &q.name, q.routine_kind.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct RefreshBody {
    /// 只刷新这个 schema；不给则刷新整个连接。
    #[serde(default)]
    pub schema: Option<String>,
}

/// 手动刷新：让元数据缓存失效。
pub async fn refresh(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<RefreshBody>,
) -> ApiResult<StatusCode> {
    s.service.profile(&id)?;
    s.service.metadata.cache().invalidate(&id, b.schema.as_deref());
    Ok(StatusCode::NO_CONTENT)
}

// ---- 会话 ----

#[derive(Deserialize)]
pub struct OpenSession {
    pub profile_id: String,
}

pub async fn open_session(
    State(s): State<AppState>,
    Json(b): Json<OpenSession>,
) -> ApiResult<(StatusCode, Json<SessionInfo>)> {
    Ok((StatusCode::CREATED, Json(s.service.open_session(&b.profile_id).await?)))
}

pub async fn sessions(State(s): State<AppState>) -> ApiResult<Json<Vec<SessionInfo>>> {
    Ok(Json(s.service.sessions.list()?))
}

pub async fn session(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<Json<SessionInfo>> {
    Ok(Json(s.service.sessions.info(id)?))
}

#[derive(Deserialize)]
pub struct CloseQuery {
    /// 有未提交的修改时：true 提交，false（默认）回滚。
    #[serde(default)]
    pub commit: bool,
}

pub async fn close_session(
    State(s): State<AppState>,
    Path(id): Path<SessionId>,
    Query(q): Query<CloseQuery>,
) -> ApiResult<StatusCode> {
    s.service.sessions.close(id, q.commit).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AutocommitBody {
    pub on: bool,
}

pub async fn set_autocommit(
    State(s): State<AppState>,
    Path(id): Path<SessionId>,
    Json(b): Json<AutocommitBody>,
) -> ApiResult<Json<SessionInfo>> {
    s.service.sessions.set_autocommit(id, b.on).await?;
    Ok(Json(s.service.sessions.info(id)?))
}

pub async fn commit(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<Json<SessionInfo>> {
    s.service.sessions.commit(id).await?;
    Ok(Json(s.service.sessions.info(id)?))
}

pub async fn rollback(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<Json<SessionInfo>> {
    s.service.sessions.rollback(id).await?;
    Ok(Json(s.service.sessions.info(id)?))
}

pub async fn reconnect(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<Json<SessionInfo>> {
    Ok(Json(s.service.reconnect_session(id).await?))
}

pub async fn cancel(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<StatusCode> {
    s.service.sessions.cancel(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
