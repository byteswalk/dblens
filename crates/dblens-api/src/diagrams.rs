//! ER 图与逻辑外键的接口（M4b 设计文档 5.2 节），以及桌面程序导出图片时写文件。都要求 `local_files`。

use std::path::PathBuf;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use dblens_core::{Error, msg};
use dblens_service::{Diagram, DiagramSummary, DiagramView, LogicalKey};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;
use crate::models::local_only;

#[derive(Deserialize)]
pub struct SchemaQuery {
    schema: String,
}

pub async fn list(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SchemaQuery>,
) -> ApiResult<Json<Vec<DiagramSummary>>> {
    local_only(&s)?;
    Ok(Json(s.service.models.diagrams(&id, &q.schema)?))
}

#[derive(Deserialize)]
pub struct NewDiagram {
    schema: String,
    name: String,
    #[serde(default)]
    tables: Vec<String>,
}

/// 新建主题图，返回 201 和 `{fingerprint}`。
pub async fn create(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<NewDiagram>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    local_only(&s)?;
    let fp = s.service.models.create_diagram(&id, &b.schema, &b.name, b.tables)?;
    Ok((StatusCode::CREATED, Json(json!({ "fingerprint": fp }))))
}

pub async fn get(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<Json<DiagramView>> {
    local_only(&s)?;
    Ok(Json(s.service.models.read_diagram(&id, &schema, &name)?))
}

#[derive(Deserialize)]
pub struct SaveBody {
    diagram: Diagram,
    #[serde(default)]
    fingerprint: Option<String>,
}

/// 保存图，返回新的 `{fingerprint}`；文件已被修改时 400。
pub async fn save(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<SaveBody>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.save_diagram(&id, &schema, &name, b.diagram, b.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

pub async fn delete(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.delete_diagram(&id, &schema, &name)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct RenameBody {
    to: String,
}

pub async fn rename(
    State(s): State<AppState>,
    Path((id, schema, name)): Path<(String, String, String)>,
    Json(b): Json<RenameBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.models.rename_diagram(&id, &schema, &name, &b.to)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct KeyBody {
    key: LogicalKey,
    #[serde(default)]
    fingerprint: Option<String>,
}

/// 加逻辑外键，返回表文件的新 `{fingerprint}`。
pub async fn add_key(
    State(s): State<AppState>,
    Path((id, schema, table)): Path<(String, String, String)>,
    Json(b): Json<KeyBody>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.add_logical_key(&id, &schema, &table, b.key, b.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

#[derive(Deserialize)]
pub struct FingerprintQuery {
    #[serde(default)]
    fingerprint: Option<String>,
}

pub async fn delete_key(
    State(s): State<AppState>,
    Path((id, schema, table, index)): Path<(String, String, String, usize)>,
    Query(q): Query<FingerprintQuery>,
) -> ApiResult<Json<Value>> {
    local_only(&s)?;
    let fp = s.service.models.delete_logical_key(&id, &schema, &table, index, q.fingerprint.as_deref())?;
    Ok(Json(json!({ "fingerprint": fp })))
}

#[derive(Deserialize)]
pub struct ImageBody {
    path: PathBuf,
    /// base64 编码的文件内容。
    data: String,
}

/// 桌面程序：把导出的 ER 图写到"另存为"选的路径（只接受 `.svg`、`.png`）。
pub async fn save_image(State(s): State<AppState>, Json(b): Json<ImageBody>) -> ApiResult<StatusCode> {
    local_only(&s)?;
    let ok =
        b.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg") || e.eq_ignore_ascii_case("png"));
    if !ok {
        return Err(Error::invalid(msg!("model.image_ext")).into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b.data.as_bytes())
        .map_err(|e| Error::invalid(msg!("model.image_data", error = e)))?;
    std::fs::write(&b.path, bytes)
        .map_err(|e| Error::internal(msg!("model.io_failed", path = b.path.display(), error = e)))?;
    Ok(StatusCode::NO_CONTENT)
}
