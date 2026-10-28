//! 模型与比对的接口（M4a 设计文档 4.4 节）。模型目录是服务所在机器上的路径：
//! 只有页面和服务在同一台机器上（`ApiConfig::local_files`）时才能使用模型，否则返回禁止。

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_core::{Error, msg};
use dblens_dialect::design::DesignCaps;
use dblens_service::{
    CompareRequest, CompareResult, ModelDesignPreview, ModelDesignRequest, ModelSummary, ModelTableView,
    NewModel, ReverseRequest, SyncPreview, SyncRequest, SyncStarted, TableEntry,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

fn local_only(s: &AppState) -> ApiResult<()> {
    if !s.config.local_files {
        return Err(Error::Denied { reason: msg!("model.local_only") }.into());
    }
    Ok(())
}

/// 打开的模型和最近打开的模型目录。
pub async fn list(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    Ok(Json(json!({ "open": s.service.models.list(), "recent": s.service.recent_models()? })))
}

#[derive(Deserialize)]
pub struct OpenBody {
    path: PathBuf,
}

pub async fn open(State(s): State<AppState>, Json(b): Json<OpenBody>) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.open_model(&b.path)?))
}

pub async fn create(
    State(s): State<AppState>,
    Json(req): Json<NewModel>,
) -> ApiResult<(StatusCode, Json<ModelSummary>)> {
    local_only(&s)?;
    Ok((StatusCode::CREATED, Json(s.service.create_model(req)?)))
}

/// 反向生成：返回任务 id，结束后任务状态的 `model` 是打开的模型。
pub async fn reverse(
    State(s): State<AppState>,
    Json(req): Json<ReverseRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    local_only(&s)?;
    let id = s.service.start_reverse(req).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 重新加载并返回模型信息。
pub async fn get(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.models.summary_of(&id)?))
}

#[derive(Deserialize)]
pub struct InfoBody {
    name: String,
    #[serde(default)]
    description: Option<String>,
    schemas: Vec<String>,
}

pub async fn update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<InfoBody>,
) -> ApiResult<Json<ModelSummary>> {
    local_only(&s)?;
    Ok(Json(s.service.models.set_info(&id, &b.name, b.description, b.schemas)?))
}

pub async fn close(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.close(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 设计器的能力和类型目录：按模型的数据库类型。
pub async fn caps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<DesignCaps>> {
    local_only(&s)?;
    Ok(Json(s.service.models.dialect(&id)?.design_caps()))
}

pub async fn tables(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Vec<TableEntry>>> {
    local_only(&s)?;
    Ok(Json(s.service.models.tables(&id)?))
}

pub async fn table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<Json<ModelTableView>> {
    local_only(&s)?;
    Ok(Json(s.service.models.read_table(&id, &schema, &name)?))
}

pub async fn preview_table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ModelDesignRequest>,
) -> ApiResult<Json<ModelDesignPreview>> {
    local_only(&s)?;
    Ok(Json(s.service.preview_model_design(&id, &req).await?))
}

/// 保存设计器中的表（新建或修改），返回新的指纹。
pub async fn save_table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ModelDesignRequest>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fingerprint = s.service.save_model_design(&id, req).await?;
    Ok(Json(json!({ "fingerprint": fingerprint })))
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    fingerprint: Option<String>,
}

pub async fn delete_table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Query(q): Query<DeleteQuery>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.delete_table(&id, &schema, &name, q.fingerprint.as_deref())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RenameBody {
    to: String,
}

pub async fn rename_table(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<RenameBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.rename_table(&id, &schema, &name, &b.to)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn compare(
    State(s): State<AppState>,
    Json(req): Json<CompareRequest>,
) -> ApiResult<Json<CompareResult>> {
    Ok(Json(s.service.compare(&req).await?))
}

/// 同步的预览：目标是库时为脚本，目标是模型时为要写的文件。
pub async fn sync_preview(
    State(s): State<AppState>,
    Json(req): Json<SyncRequest>,
) -> ApiResult<Json<SyncPreview>> {
    Ok(Json(s.service.preview_sync(&req).await?))
}

#[derive(Deserialize)]
pub struct ScriptBody {
    path: PathBuf,
    text: String,
}

/// 桌面程序：把同步脚本写到"另存为"选的路径（只接受 `.sql` 文件）。
pub async fn save_script(State(s): State<AppState>, Json(b): Json<ScriptBody>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    if !b.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")) {
        return Err(Error::invalid(msg!("model.script_ext")).into());
    }
    std::fs::write(&b.path, b.text)
        .map_err(|e| Error::internal(msg!("model.io_failed", path = b.path.display(), error = e)))?;
    Ok(StatusCode::NO_CONTENT)
}

/// 同步：目标是库时返回任务 id（201），目标是模型时直接写回（200，`task` 为 null）。
pub async fn sync(
    State(s): State<AppState>,
    Json(req): Json<SyncRequest>,
) -> ApiResult<(StatusCode, Json<SyncStarted>)> {
    let started = s.service.start_sync(req).await?;
    let status = if started.task.is_some() { StatusCode::CREATED } else { StatusCode::OK };
    Ok((status, Json(started)))
}
