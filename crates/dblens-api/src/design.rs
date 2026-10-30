//! 表设计器的接口（M3 设计文档 6.1 节）：方言能力和类型目录、读原表、预览、执行（返回任务 id，状态用 `/tasks/{id}`）。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use dblens_dialect::design::DesignCaps;
use dblens_service::{DesignPreview, DesignRequest, DesignTable};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

pub async fn caps(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<DesignCaps>> {
    Ok(Json(s.service.design_caps(&id).await?))
}

#[derive(Deserialize)]
pub struct TableQuery {
    #[serde(default)]
    catalog: Option<String>,
    schema: String,
    name: String,
}

/// 原表（直接从库读取，不走缓存）、存储属性和行数估计、指纹。
pub async fn table(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<TableQuery>,
) -> ApiResult<Json<DesignTable>> {
    Ok(Json(s.service.design_table(&id, q.catalog.as_deref(), &q.schema, &q.name).await?))
}

pub async fn preview(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DesignRequest>,
) -> ApiResult<Json<DesignPreview>> {
    Ok(Json(s.service.preview_design(&id, &req).await?))
}

/// 执行：后端用草稿重新生成语句，作为任务运行。
pub async fn apply(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<DesignRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let task = s.service.start_design(&id, req).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": task }))))
}
