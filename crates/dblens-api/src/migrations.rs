//! 结构迁移的接口（M5a 设计文档 3.2 节）：预览（转换结果、脚本、开始前检查），开始迁移。

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use dblens_service::{MigrationPlan, MigrationPreview};
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
