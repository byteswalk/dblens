//! 文档导出的接口（M4c 设计文档 6.2 节）。本机路径（模板、样式来源、输出位置）只有 `local_files` 时接受；
//! 浏览器模式用内置模板，完成后从任务的报告下载。

use std::path::PathBuf;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dblens_service::document::{DocumentRequest, TextFormat};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;
use crate::models::local_only;

/// 开始导出：返回任务 id。
pub async fn start(
    State(s): State<AppState>,
    Json(req): Json<DocumentRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_document(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 开始前检查：模板语法、样式来源、输出位置。
pub async fn check(State(s): State<AppState>, Json(req): Json<DocumentRequest>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "problems": s.service.check_document(&req, s.config.local_files) })))
}

#[derive(Deserialize)]
pub struct TemplateBody {
    format: TextFormat,
    path: PathBuf,
}

/// 把内置模板写到指定路径。
pub async fn export_template(
    State(s): State<AppState>,
    Json(b): Json<TemplateBody>,
) -> ApiResult<StatusCode> {
    local_only(&s)?;
    s.service.export_document_template(b.format, &b.path)?;
    Ok(StatusCode::NO_CONTENT)
}
