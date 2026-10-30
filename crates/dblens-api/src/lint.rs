//! 设计规范检查的接口（M7a、M7b）。规范库、发起检查、人工结论、例外、导出报告。

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use dblens_core::{Error, msg};
use dblens_lint::{RuleInfo, Ruleset};
use dblens_service::{
    LintRequest, LintRun, LintRunSummary, ReportFormat, ReviewInput, RulesetSummary, WaiveInput,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiResult;

pub async fn rulesets(State(s): State<AppState>) -> ApiResult<Json<Vec<RulesetSummary>>> {
    Ok(Json(s.service.lint_rulesets()?))
}

pub async fn create_ruleset(
    State(s): State<AppState>,
    Json(set): Json<Ruleset>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.create_lint_ruleset(set)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn ruleset(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Ruleset>> {
    Ok(Json(s.service.lint_ruleset(&id)?))
}

pub async fn update_ruleset(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(set): Json<Ruleset>,
) -> ApiResult<StatusCode> {
    s.service.update_lint_ruleset(&id, set)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_ruleset(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.delete_lint_ruleset(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn reset_ruleset(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Ruleset>> {
    Ok(Json(s.service.reset_lint_ruleset(&id)?))
}

pub async fn export_ruleset(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let yaml = s.service.export_lint_ruleset(&id)?;
    let disposition = format!("attachment; filename=\"{id}.yaml\"");
    Ok((
        [
            (header::CONTENT_TYPE, "application/yaml; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        yaml,
    ))
}

#[derive(Deserialize)]
pub struct ImportBody {
    yaml: String,
}

pub async fn import_ruleset(
    State(s): State<AppState>,
    Json(b): Json<ImportBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.import_lint_ruleset(&b.yaml)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn rules(State(s): State<AppState>) -> ApiResult<Json<Vec<RuleInfo>>> {
    Ok(Json(s.service.lint_rule_catalog()))
}

pub async fn runs(State(s): State<AppState>) -> ApiResult<Json<Vec<LintRunSummary>>> {
    Ok(Json(s.service.lint_runs()?))
}

pub async fn start_run(
    State(s): State<AppState>,
    Json(req): Json<LintRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_lint(req).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<LintRun>> {
    Ok(Json(s.service.lint_run(&id)?))
}

pub async fn delete_run(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.delete_lint_run(&id)?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn review(
    State(s): State<AppState>,
    Path((id, clause)): Path<(String, String)>,
    Json(input): Json<ReviewInput>,
) -> ApiResult<Json<LintRun>> {
    Ok(Json(s.service.review_lint_clause(&id, &clause, input)?))
}

pub async fn waive(
    State(s): State<AppState>,
    Path((id, clause)): Path<(String, String)>,
    Json(input): Json<WaiveInput>,
) -> ApiResult<Json<LintRun>> {
    Ok(Json(s.service.waive_lint_issue(&id, &clause, input)?))
}

#[derive(Deserialize)]
pub struct UnwaiveQuery {
    object: String,
}

pub async fn unwaive(
    State(s): State<AppState>,
    Path((id, clause)): Path<(String, String)>,
    Query(q): Query<UnwaiveQuery>,
) -> ApiResult<Json<LintRun>> {
    Ok(Json(s.service.unwaive_lint_issue(&id, &clause, &q.object)?))
}

#[derive(Deserialize)]
pub struct ReportQuery {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    reviewer: Option<String>,
}

pub async fn report(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<ReportQuery>,
) -> ApiResult<impl IntoResponse> {
    let fmt = q.format.as_deref().unwrap_or("html");
    let format =
        ReportFormat::parse(fmt).ok_or_else(|| Error::invalid(msg!("api.bad_format", format = fmt)))?;
    let (bytes, name) = s.service.lint_report(&id, q.reviewer, format)?;
    let disposition = format!("attachment; filename=\"{name}\"");
    Ok((
        [
            (header::CONTENT_TYPE, format.content_type().to_string()),
            (header::CONTENT_DISPOSITION, disposition),
        ],
        bytes,
    ))
}
