//! 数据库账号与权限接口（M9 设计文档第 7 节）。只读。

use axum::Json;
use axum::extract::{Path, Query, State};
use dblens_service::{ObjectAccessReport, PrincipalPrivileges, PrivilegeOverview};
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiResult;

#[derive(Deserialize)]
pub struct CatalogQuery {
    catalog: Option<String>,
}

#[derive(Deserialize)]
pub struct PrincipalQuery {
    principal: String,
    catalog: Option<String>,
}

#[derive(Deserialize)]
pub struct ObjectQuery {
    schema: String,
    object: String,
    catalog: Option<String>,
}

pub async fn overview(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<CatalogQuery>,
) -> ApiResult<Json<PrivilegeOverview>> {
    Ok(Json(s.service.privilege_overview(&id, q.catalog.as_deref()).await?))
}

pub async fn principal(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<PrincipalQuery>,
) -> ApiResult<Json<PrincipalPrivileges>> {
    Ok(Json(s.service.principal_privileges(&id, q.catalog.as_deref(), &q.principal).await?))
}

pub async fn object(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ObjectQuery>,
) -> ApiResult<Json<ObjectAccessReport>> {
    Ok(Json(s.service.object_access(&id, q.catalog.as_deref(), &q.schema, &q.object).await?))
}
