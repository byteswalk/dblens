//! 导入与执行 SQL 文件的接口（M2b 设计文档第 8 节）：上传文件、预览、建表语句、提交任务、任务状态、确认、取消、报告。

use std::path::PathBuf;

use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use dblens_core::{Error, msg};
use dblens_service::{
    FileOptions, FilePreview, FileSource, ImportRequest, NewTable, SqlFileRequest, TaskStatus, UploadInfo,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::AppState;
use crate::error::ApiResult;
use crate::exports::stream_file;

/// 上传大小上限（MB）的设置项。
pub const UPLOAD_LIMIT_SETTING: &str = "ui.import.upload_limit_mb";
/// 默认上传上限 2 GB。
pub const DEFAULT_UPLOAD_LIMIT_MB: u64 = 2048;

#[derive(Deserialize)]
pub struct UploadQuery {
    name: String,
}

/// 上传文件：请求体是文件内容，`?name=` 给出原文件名。边收边写到数据目录的 `imports/` 下。
pub async fn upload(
    State(s): State<AppState>,
    Query(q): Query<UploadQuery>,
    body: Body,
) -> ApiResult<(StatusCode, Json<UploadInfo>)> {
    let limit_mb = s
        .service
        .store
        .setting(UPLOAD_LIMIT_SETTING)?
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_UPLOAD_LIMIT_MB);
    let limit = limit_mb.saturating_mul(1024 * 1024);
    let (id, path) = s.service.tasks.begin_upload()?;
    let failed = |e: &dyn std::fmt::Display| Error::internal(msg!("import.upload_failed", error = e));
    let result = async {
        let mut file = tokio::fs::File::create(&path).await.map_err(|e| failed(&e))?;
        let mut size = 0u64;
        let mut stream = body.into_data_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| failed(&e))?;
            size += chunk.len() as u64;
            if size > limit {
                return Err(Error::invalid(msg!("import.too_large", limit = limit_mb)));
            }
            file.write_all(&chunk).await.map_err(|e| failed(&e))?;
        }
        file.flush().await.map_err(|e| failed(&e))?;
        Ok(size)
    }
    .await;
    match result {
        Ok(size) => Ok((StatusCode::CREATED, Json(s.service.tasks.finish_upload(&id, &q.name, size)?))),
        Err(e) => {
            let _ = tokio::fs::remove_file(&path).await;
            Err(e.into())
        }
    }
}

#[derive(Deserialize)]
pub struct PreviewBody {
    source: FileSource,
    file: FileOptions,
}

/// 预览文件：列名、前 100 行、推断的类型。
pub async fn preview(State(s): State<AppState>, Json(b): Json<PreviewBody>) -> ApiResult<Json<FilePreview>> {
    Ok(Json(s.service.preview_import(&b.source, &b.file, s.config.local_files).await?))
}

#[derive(Deserialize)]
pub struct DdlBody {
    schema: String,
    table: String,
    create: NewTable,
}

/// 新建表的建表语句预览。
pub async fn ddl(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<DdlBody>,
) -> ApiResult<Json<Value>> {
    let sql = s.service.import_ddl(&id, &b.schema, &b.table, &b.create).await?;
    Ok(Json(json!({ "sql": sql })))
}

/// 提交导入任务。只有桌面程序的本机服务接受本机路径。
pub async fn import(
    State(s): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_import(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

/// 提交执行 SQL 文件的任务。
pub async fn sql_file(
    State(s): State<AppState>,
    Json(req): Json<SqlFileRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_sql_file(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<TaskStatus>> {
    Ok(Json(s.service.tasks.status(&id)?))
}

/// 进行中的任务取消；已结束的任务删除记录和报告。
pub async fn cancel_or_remove(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.tasks.cancel_or_remove(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ConfirmBody {
    yes: bool,
}

pub async fn confirm(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ConfirmBody>,
) -> ApiResult<StatusCode> {
    s.service.tasks.confirm(&id, b.yes)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 下载报告（出错报告或执行日志；链接中用 `?token=` 带令牌）。报告在任务过期前可以多次下载。
pub async fn report(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let (path, name) = s.service.tasks.report(&id)?;
    Ok(stream_file(path, &name, false).await?)
}

#[derive(Deserialize)]
pub struct SaveBody {
    path: PathBuf,
}

/// 桌面程序：把报告复制到"另存为"选的路径。
pub async fn save_report(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<SaveBody>,
) -> ApiResult<StatusCode> {
    if !s.config.local_files {
        return Err(Error::invalid(msg!("export.path_not_allowed")).into());
    }
    s.service.tasks.save_report(&id, &b.path)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 前 100 条失败中第 `n` 条的完整语句。
pub async fn statement(
    State(s): State<AppState>,
    Path((id, n)): Path<(String, usize)>,
) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "sql": s.service.tasks.statement(&id, n)? })))
}
