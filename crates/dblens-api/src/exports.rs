//! 导出与运维查询接口（M1c3 设计文档 3.1、5.2 节）。

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use dblens_core::{Lang, current_lang};
use dblens_dialect::OpsCategory;
use dblens_service::{ExportRequest, ExportStatus};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::io::AsyncReadExt;

use crate::AppState;
use crate::error::ApiResult;

/// 提交导出任务。只有桌面程序的本机服务（`ApiConfig::local_files`）接受保存路径。
pub async fn create(
    State(s): State<AppState>,
    Json(req): Json<ExportRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let id = s.service.start_export(req, s.config.local_files).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

pub async fn status(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<ExportStatus>> {
    Ok(Json(s.service.exports.status(&id)?))
}

/// 进行中的任务取消；已结束的任务删除记录和临时文件。
pub async fn cancel_or_remove(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    s.service.exports.cancel_or_remove(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 传完（或连接中断）后删除临时文件。
struct RemoveOnDrop(std::path::PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `Content-Disposition` 的文件名：RFC 5987 的 `filename*`，非 ASCII 字符按 UTF-8 百分号编码；另给一个 ASCII 的后备名。
fn disposition(name: &str) -> HeaderValue {
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let ascii: String =
        name.chars().map(|c| if c.is_ascii_alphanumeric() || "-._".contains(c) { c } else { '_' }).collect();
    HeaderValue::from_str(&format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// 浏览器模式下载导出的文件（链接中用 `?token=` 带令牌）。下载后任务不再存在。
pub async fn file(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Response> {
    let (path, name) = s.service.exports.take_file(&id)?;
    let guard = RemoveOnDrop(path.clone());
    let f = tokio::fs::File::open(&path)
        .await
        .map_err(|e| dblens_core::Error::internal(dblens_core::msg!("export.write_failed", error = e)))?;
    // 文件和删除守卫一起放在流的状态里：流结束或被丢弃时先关闭文件，再删除
    let stream = futures::stream::unfold(Some((f, guard)), |state| async move {
        let (mut f, guard) = state?;
        let mut buf = vec![0u8; 64 * 1024];
        match f.read(&mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(Bytes::from(buf)), Some((f, guard))))
            }
            Err(e) => Some((Err(e), None)),
        }
    });
    let mut resp = Body::from_stream(stream).into_response();
    resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    resp.headers_mut().insert(header::CONTENT_DISPOSITION, disposition(&name));
    Ok(resp)
}

#[derive(Serialize)]
pub struct OpsItem {
    key: &'static str,
    /// 按请求的语言。
    name: &'static str,
    category: OpsCategory,
    sql: &'static str,
}

/// 连接可用的运维查询。
pub async fn ops(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Vec<OpsItem>>> {
    let en = current_lang() == Lang::En;
    let items = s
        .service
        .ops(&id)?
        .iter()
        .map(|q| OpsItem {
            key: q.key.as_str(),
            name: if en { q.name_en.as_str() } else { q.name_zh.as_str() },
            category: q.category,
            sql: q.sql.as_str(),
        })
        .collect();
    Ok(Json(items))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_encodes_non_ascii() {
        assert_eq!(
            disposition("订单 2026.csv").to_str().unwrap(),
            "attachment; filename=\"___2026.csv\"; filename*=UTF-8''%E8%AE%A2%E5%8D%95%202026.csv"
        );
    }
}
