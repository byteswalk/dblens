//! REST 接口。路径都在 `/api` 下，请求和响应都是 JSON；出错时的响应体见 `error.rs`。

use std::collections::BTreeMap;
use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_core::schema::{Routine, Sequence, Synonym, Table, View};
use dblens_core::{Error, msg};
use dblens_dialect::meta::{MetaKey, ObjectInfo, SchemaInfo};
use dblens_dialect::{DdlKind, Relation};
use dblens_driver::DriverId;
use dblens_service::{
    ApplyOutcome, ChangeSet, DataOrder, DriverStatus, Favorite, FavoritePatch, GenerateKind, HistoryItem,
    HistoryQuery, NewFavorite, ObjectKind, Profile, ProfileInput, SearchHit, SecretStatus, ServerSummary,
    SessionId, SessionInfo,
};
use serde::Deserialize;
use serde_json::{Value, json};

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

// ---- 设置 ----

/// 单个设置项的值上限。设置是界面状态（主题、面板大小、打开的标签页等），不应很大。
const MAX_SETTING_BYTES: usize = 64 * 1024;

/// 这个接口只读写界面设置（`ui.` 开头）。同一张表里还有服务层自己的设置（如已确认的原生库目录 `native.*`），
/// 它们有各自的接口和校验，不能经这里绕过。
const UI_PREFIX: &str = "ui.";

/// 设置项名称：`ui.` 开头，共 1–64 个小写字母、数字或 `.`、`_`、`-`，如 `ui.theme`。
fn valid_setting_key(key: &str) -> bool {
    key.starts_with(UI_PREFIX)
        && key.len() <= 64
        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
}

pub async fn settings(State(s): State<AppState>) -> ApiResult<Json<BTreeMap<String, String>>> {
    let mut all = s.service.store.settings()?;
    all.retain(|k, _| k.starts_with(UI_PREFIX));
    Ok(Json(all))
}

#[derive(Deserialize)]
pub struct SettingBody {
    /// null 表示删除这一项。
    pub value: Option<String>,
}

#[derive(Deserialize)]
pub struct ClientLog {
    pub kind: String,
    pub message: String,
    pub stack: Option<String>,
    pub component_stack: Option<String>,
}

/// 前端的错误写进日志：界面崩溃在日志里能查到。
pub async fn client_log(Json(b): Json<ClientLog>) -> StatusCode {
    const MAX_CHARS: usize = 8000;
    let cut = |s: Option<String>| s.unwrap_or_default().chars().take(MAX_CHARS).collect::<String>();
    tracing::error!(
        kind = %b.kind,
        message = %cut(Some(b.message)),
        stack = %cut(b.stack),
        component_stack = %cut(b.component_stack),
        "client error"
    );
    StatusCode::NO_CONTENT
}

pub async fn set_setting(
    State(s): State<AppState>,
    Path(key): Path<String>,
    Json(b): Json<SettingBody>,
) -> ApiResult<StatusCode> {
    if !valid_setting_key(&key) {
        return Err(Error::invalid(msg!("api.bad_setting_key", key = key)).into());
    }
    if b.value.as_ref().is_some_and(|v| v.len() > MAX_SETTING_BYTES) {
        return Err(Error::invalid(msg!("api.setting_too_large", key = key)).into());
    }
    s.service.store.set_setting(&key, b.value.as_deref())?;
    if key == dblens_service::LANG_SETTING {
        s.service.apply_lang_setting()?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---- 历史 ----

pub async fn history(
    State(s): State<AppState>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<Json<Vec<HistoryItem>>> {
    Ok(Json(s.service.store.history(&q)?))
}

pub async fn delete_history(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    s.service.store.delete_history(id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn clear_history(State(s): State<AppState>) -> ApiResult<StatusCode> {
    s.service.store.clear_history()?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- 收藏 ----

pub async fn favorites(State(s): State<AppState>) -> ApiResult<Json<Vec<Favorite>>> {
    Ok(Json(s.service.store.favorites()?))
}

pub async fn create_favorite(
    State(s): State<AppState>,
    Json(b): Json<NewFavorite>,
) -> ApiResult<(StatusCode, Json<Favorite>)> {
    Ok((StatusCode::CREATED, Json(s.service.store.create_favorite(b)?)))
}

pub async fn update_favorite(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<FavoritePatch>,
) -> ApiResult<Json<Favorite>> {
    Ok(Json(s.service.store.update_favorite(&id, b)?))
}

pub async fn delete_favorite(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.store.delete_favorite(&id)?;
    Ok(StatusCode::NO_CONTENT)
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

/// 所在的数据库（SQL Server 的连接 → 数据库 → 架构）。不给时用连接配置里的初始库。
#[derive(Deserialize)]
pub struct CatalogQuery {
    pub catalog: Option<String>,
}

/// 数据库列表（只有 SQL Server 有这一层，其他驱动返回空数组）。
pub async fn catalogs(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<FilterQuery>,
) -> ApiResult<Json<Vec<SchemaInfo>>> {
    let p = s.service.profile(&id)?;
    Ok(Json(s.service.metadata.catalogs(&p, q.filter.as_deref()).await?))
}

pub async fn schemas(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<FilterQuery>,
) -> ApiResult<Json<Vec<SchemaInfo>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
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
    Query(c): Query<CatalogQuery>,
    Query(q): Query<ObjectsQuery>,
) -> ApiResult<Json<Vec<ObjectInfo>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
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
    Query(c): Query<CatalogQuery>,
    Query(q): Query<TablesQuery>,
) -> ApiResult<Json<Vec<Table>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.tables(&p, &schema, q.table.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

pub async fn search(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<SearchQuery>,
) -> ApiResult<Json<Vec<SearchHit>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.search(&p, &schema, &q.q).await?))
}

#[derive(Deserialize)]
pub struct ViewsQuery {
    /// views / materialized_views，默认 views。
    pub kind: Option<MetaKey>,
}

pub async fn views(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<ViewsQuery>,
) -> ApiResult<Json<Vec<View>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    let kind = q.kind.unwrap_or(MetaKey::Views);
    if !matches!(kind, MetaKey::Views | MetaKey::MaterializedViews) {
        return Err(Error::invalid(msg!("api.bad_view_kind")).into());
    }
    Ok(Json(s.service.metadata.views(&p, &schema, kind).await?))
}

pub async fn sequences(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
) -> ApiResult<Json<Vec<Sequence>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.sequences(&p, &schema).await?))
}

pub async fn routines(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
) -> ApiResult<Json<Vec<Routine>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.routines(&p, &schema).await?))
}

pub async fn synonyms(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
) -> ApiResult<Json<Vec<Synonym>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
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
    Query(c): Query<CatalogQuery>,
    Query(q): Query<SourceQuery>,
) -> ApiResult<Json<Option<String>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.source(&p, &schema, q.kind, &q.name, q.routine_kind.as_deref()).await?))
}

#[derive(Deserialize)]
pub struct RefreshBody {
    /// 只刷新这个 schema；不给则刷新整个连接。
    #[serde(default)]
    pub schema: Option<String>,
}

// ---- 对象操作（M1c2） ----

#[derive(Deserialize)]
pub struct DdlQuery {
    /// table / view / materialized_view / sequence / function / procedure / package / package_body / synonym。
    pub kind: DdlKind,
    pub name: String,
}

pub async fn ddl(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<DdlQuery>,
) -> ApiResult<Json<Value>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    let ddl = s.service.metadata.ddl(&p, &schema, q.kind, &q.name).await?;
    Ok(Json(json!({ "ddl": ddl })))
}

#[derive(Deserialize)]
pub struct GenerateQuery {
    pub object: String,
    /// table / view / materialized_view，默认 table。
    #[serde(default)]
    pub object_kind: ObjectKind,
    /// select / insert / update / delete / join。
    pub kind: GenerateKind,
    /// 关联查询只连接这张表；两个都不给时连接本表引用的全部表。
    pub ref_schema: Option<String>,
    pub ref_table: Option<String>,
}

pub async fn generate(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<GenerateQuery>,
) -> ApiResult<Json<Value>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    let reference = q.ref_table.as_deref().map(|t| (q.ref_schema.as_deref().unwrap_or(&schema), t));
    let sql = s.service.metadata.generate(&p, &schema, &q.object, q.object_kind, q.kind, reference).await?;
    Ok(Json(json!({ "sql": sql })))
}

#[derive(Deserialize)]
pub struct RelationsQuery {
    pub table: String,
}

pub async fn relations(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Query(q): Query<RelationsQuery>,
) -> ApiResult<Json<Vec<Relation>>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    Ok(Json(s.service.metadata.relations(&p, &schema, &q.table).await?))
}

#[derive(Deserialize)]
pub struct DataSqlBody {
    pub object: String,
    /// WHERE 后的条件，原样拼入。
    pub filter: Option<String>,
    pub order: Option<DataOrder>,
    /// true 时生成 `SELECT COUNT(*)`。
    #[serde(default)]
    pub count: bool,
}

pub async fn data_sql(
    State(s): State<AppState>,
    Path((id, schema)): Path<(String, String)>,
    Query(c): Query<CatalogQuery>,
    Json(b): Json<DataSqlBody>,
) -> ApiResult<Json<Value>> {
    let p = s.service.profile(&id)?.in_catalog(c.catalog.as_deref());
    let filter = b.filter.as_deref().map(str::trim).filter(|f| !f.is_empty());
    let sql = s.service.metadata.data_sql(&p, &schema, &b.object, filter, b.order.as_ref(), b.count).await?;
    Ok(Json(json!({ "sql": sql })))
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
pub struct SchemaBody {
    pub schema: String,
}

/// 切换会话的当前 schema。
pub async fn set_schema(
    State(s): State<AppState>,
    Path(id): Path<SessionId>,
    Json(b): Json<SchemaBody>,
) -> ApiResult<Json<SessionInfo>> {
    s.service.sessions.set_schema(id, &b.schema).await?;
    Ok(Json(s.service.sessions.info(id)?))
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

// ---- 表数据编辑（M2a） ----

/// 预览改动集生成的语句：`[{index, sql}]`，`sql` 是值写成字面量的形式，不执行。
pub async fn preview_changes(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(set): Json<ChangeSet>,
) -> ApiResult<Json<Vec<Value>>> {
    let statements = s.service.preview_changes(&id, &set).await?;
    Ok(Json(statements.into_iter().map(|st| json!({ "index": st.index, "sql": st.preview })).collect()))
}

/// 在会话中执行改动集。失败时整体回滚，结果中 `failed` 带失败的改动序号和错误（仍是 200）。
pub async fn apply_changes(
    State(s): State<AppState>,
    Path(id): Path<SessionId>,
    Json(set): Json<ChangeSet>,
) -> ApiResult<Json<ApplyOutcome>> {
    Ok(Json(s.service.apply_changes(id, &set).await?))
}

pub async fn cancel(State(s): State<AppState>, Path(id): Path<SessionId>) -> ApiResult<StatusCode> {
    s.service.sessions.cancel(id).await?;
    Ok(StatusCode::NO_CONTENT)
}
