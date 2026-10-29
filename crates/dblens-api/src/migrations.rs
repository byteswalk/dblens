//! 迁移的接口（M5a 设计文档 3.2 节、M5b 设计文档第 7 节）：预览（转换结果、脚本、开始前检查）、开始迁移；
//! 没有完成的迁移：列出、继续、放弃。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use dblens_service::{MigrationPlan, MigrationPreview, MigrationSummary};
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

/// 预览：读结构、转换、检查、生成脚本。
pub async fn preview(
    State(s): State<AppState>,
    Json(plan): Json<MigrationPlan>,
) -> ApiResult<Json<MigrationPreview>> {
    Ok(Json(s.service.preview_migration(&plan).await?))
}

/// 开始迁移：返回任务 id。
pub async fn start(
    State(s): State<AppState>,
    Json(plan): Json<MigrationPlan>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_migration(plan).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 没有完成的迁移。
pub async fn unfinished(State(s): State<AppState>) -> ApiResult<Json<Vec<MigrationSummary>>> {
    Ok(Json(s.service.unfinished_migrations()?))
}

/// 继续一个没有完成的迁移：返回任务 id。
pub async fn resume(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let task = s.service.resume_migration(&id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": task }))))
}

/// 放弃：删除迁移记录（不动目标库）。
pub async fn discard(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.discard_migration(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
