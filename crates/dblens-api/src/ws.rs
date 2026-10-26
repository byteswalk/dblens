//! 会话的 WebSocket 执行协议：`GET /api/sessions/{id}/ws`。
//!
//! 客户端发送（JSON，按 `type` 区分）：
//! - `{"type":"exec","sql":"…","confirmed":false,"max_rows":500}`：执行一段 SQL（可以多条语句）。`confirmed` 只对
//!   第一条语句有效（确认后从那条重新提交），后面需要确认的语句仍会停下。可选字段：
//!   - `continue_on_error`：某条语句失败后继续执行后面的；
//!   - `cursor`：只执行这个 UTF-16 偏移所在的那条语句；
//!   - `explain`：只生成执行计划（只能有一条语句，或用 `cursor` 选出一条）；
//! - `{"type":"fetch_more","max_rows":500}`：取下一批行；
//! - `{"type":"cancel"}`：取消正在执行的语句。
//!
//! 服务端发送 `SessionEvent`（`statement_start`、`result_start`、`rows`、`result_end`、`affected`、`message`、
//! `statement_end`、`confirm_required`、`failed`、`done`），协议层面的错误发 `{"type":"error","error":{…}}`。
//! 每发出一个装满 `max_rows` 行的 `rows` 事件就停下，等客户端 `fetch_more` 后再取下一批（背压）；
//! 不满一批说明结果集已取完，接着发后面的事件。其他事件不受限制。
//! 事件中的 `start`/`end`、`failed` 的 `offset` 是 UTF-16 代码单元偏移（与 Monaco 编辑器一致），不是字节偏移。
//! 连接断开时，正在执行的语句会被取消，剩余事件被丢弃，会话恢复空闲。

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use dblens_core::{Error, msg};
use dblens_service::{ExecRequest, Service, SessionEvent, SessionId};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::AppState;
use crate::error::ApiResult;

/// 未指定时每批最多的行数。
pub const DEFAULT_MAX_ROWS: usize = 500;

fn default_rows() -> usize {
    DEFAULT_MAX_ROWS
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Exec {
        sql: String,
        #[serde(default)]
        confirmed: bool,
        #[serde(default = "default_rows")]
        max_rows: usize,
        #[serde(default)]
        continue_on_error: bool,
        /// UTF-16 偏移。
        #[serde(default)]
        cursor: Option<usize>,
        #[serde(default)]
        explain: bool,
    },
    FetchMore {
        #[serde(default = "default_rows")]
        max_rows: usize,
    },
    Cancel,
}

/// 字节偏移换算成 UTF-16 偏移。
pub fn utf16_offset(text: &str, byte: usize) -> usize {
    text.get(..byte.min(text.len())).map_or(0, |s| s.encode_utf16().count())
}

/// UTF-16 偏移换算成字节偏移；落在字符中间时取该字符的起点，超出末尾时取文本长度。
pub fn byte_offset(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        units += c.len_utf16();
        if units > utf16 {
            return byte;
        }
    }
    text.len()
}

/// 把事件中的字节偏移换成 UTF-16 偏移后序列化。
fn encode(event: SessionEvent, sql: &str) -> String {
    let event = match event {
        SessionEvent::StatementStart { index, total, start, end, info } => SessionEvent::StatementStart {
            index,
            total,
            start: utf16_offset(sql, start),
            end: utf16_offset(sql, end),
            info,
        },
        SessionEvent::ConfirmRequired { index, start, end, reason } => SessionEvent::ConfirmRequired {
            index,
            start: utf16_offset(sql, start),
            end: utf16_offset(sql, end),
            reason,
        },
        SessionEvent::Failed { index, error, offset } => {
            SessionEvent::Failed { index, error, offset: offset.map(|o| utf16_offset(sql, o)) }
        }
        other => other,
    };
    serde_json::to_string(&event).unwrap_or_else(|e| error_message(&Error::internal(e.to_string())))
}

fn error_message(e: &Error) -> String {
    serde_json::json!({"type": "error", "error": e}).to_string()
}

pub async fn upgrade(
    State(s): State<AppState>,
    Path(id): Path<SessionId>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    s.service.sessions.info(id)?;
    let service = s.service.clone();
    Ok(ws.on_upgrade(move |socket| run(socket, service, id)))
}

/// 从会话取事件发给客户端；每发出装满的一批行就等一次 `fetch_more`。
async fn pump(
    service: Arc<Service>,
    id: SessionId,
    sql: String,
    mut max_rows: usize,
    out: mpsc::Sender<String>,
    mut credit: mpsc::Receiver<usize>,
) {
    loop {
        match service.sessions.next(id, max_rows).await {
            Ok(Some(event)) => {
                // 装满一批才等 fetch_more：不满说明结果集已经取完，接着发后面的事件
                let full = matches!(&event, SessionEvent::Rows { rows } if rows.len() >= max_rows);
                let done = event == SessionEvent::Done;
                let _ = out.send(encode(event, &sql)).await;
                if done {
                    return;
                }
                if full {
                    match credit.recv().await {
                        Some(n) => max_rows = n.max(1),
                        None => break,
                    }
                }
            }
            Ok(None) => return,
            Err(e) => {
                let _ = out.send(error_message(&e)).await;
                return;
            }
        }
    }
    // 客户端已断开：取消并取完剩余事件，让会话回到空闲
    let _ = service.sessions.cancel(id).await;
    while let Ok(Some(event)) = service.sessions.next(id, 1000).await {
        if event == SessionEvent::Done {
            break;
        }
    }
}

async fn run(socket: WebSocket, service: Arc<Service>, id: SessionId) {
    let (mut sink, mut stream) = socket.split();
    let (out, mut outbox) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(text) = outbox.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let mut credit: Option<mpsc::Sender<usize>> = None;
    let mut running: Option<tokio::task::JoinHandle<()>> = None;

    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        let msg: ClientMsg = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                let _ = out.send(error_message(&Error::invalid(msg!("api.bad_message", error = e)))).await;
                continue;
            }
        };
        match msg {
            ClientMsg::Exec { sql, confirmed, max_rows, continue_on_error, cursor, explain } => {
                if running.as_ref().is_some_and(|h| !h.is_finished()) {
                    let busy = Error::Busy { message: msg!("api.still_running") };
                    let _ = out.send(error_message(&busy)).await;
                    continue;
                }
                let cursor = cursor.map(|c| byte_offset(&sql, c));
                let req = ExecRequest { sql: sql.clone(), confirmed, continue_on_error, cursor, explain };
                if let Err(e) = service.sessions.submit(id, req).await {
                    let _ = out.send(error_message(&e)).await;
                    continue;
                }
                let (tx, rx) = mpsc::channel(1);
                credit = Some(tx);
                running =
                    Some(tokio::spawn(pump(service.clone(), id, sql, max_rows.max(1), out.clone(), rx)));
            }
            ClientMsg::FetchMore { max_rows } => {
                if let Some(c) = &credit {
                    let _ = c.try_send(max_rows);
                }
            }
            ClientMsg::Cancel => {
                if let Err(e) = service.sessions.cancel(id).await {
                    let _ = out.send(error_message(&e)).await;
                }
            }
        }
    }

    // 连接断开：还在执行就取消（pump 会取完剩余事件）
    if running.as_ref().is_some_and(|h| !h.is_finished()) {
        let _ = service.sessions.cancel(id).await;
    }
    drop(credit);
    if let Some(h) = running {
        let _ = h.await;
    }
    drop(out);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets() {
        let sql = "SELECT '数洞😀';\nSELECT 2";
        let second = sql.find("SELECT 2").unwrap();
        // 汉字各 1 个代码单元，emoji 2 个；字节偏移比 UTF-16 偏移大
        assert_eq!(utf16_offset(sql, second), "SELECT '数洞😀';\n".encode_utf16().count());
        assert!(second > utf16_offset(sql, second));
        assert_eq!(utf16_offset(sql, 0), 0);
        assert_eq!(utf16_offset(sql, 10_000), sql.encode_utf16().count());
    }

    #[test]
    fn byte_offsets_round_trip() {
        let sql = "SELECT '数洞😀';\nSELECT 2";
        for (byte, _) in sql.char_indices() {
            assert_eq!(byte_offset(sql, utf16_offset(sql, byte)), byte);
        }
        assert_eq!(byte_offset(sql, 10_000), sql.len());
        // emoji 的两个代码单元之间：取 emoji 的起点
        let emoji = sql.find('😀').unwrap();
        assert_eq!(byte_offset(sql, utf16_offset(sql, emoji) + 1), emoji);
    }

    #[test]
    fn parses_client_messages() {
        let m: ClientMsg = serde_json::from_str(r#"{"type":"exec","sql":"SELECT 1"}"#).unwrap();
        assert!(matches!(
            m,
            ClientMsg::Exec {
                confirmed: false,
                max_rows: DEFAULT_MAX_ROWS,
                continue_on_error: false,
                cursor: None,
                explain: false,
                ..
            }
        ));
        let m: ClientMsg = serde_json::from_str(
            r#"{"type":"exec","sql":"x","continue_on_error":true,"cursor":3,"explain":true}"#,
        )
        .unwrap();
        assert!(matches!(m, ClientMsg::Exec { continue_on_error: true, cursor: Some(3), explain: true, .. }));
        let m: ClientMsg = serde_json::from_str(r#"{"type":"fetch_more","max_rows":10}"#).unwrap();
        assert!(matches!(m, ClientMsg::FetchMore { max_rows: 10 }));
        assert!(matches!(
            serde_json::from_str::<ClientMsg>(r#"{"type":"cancel"}"#).unwrap(),
            ClientMsg::Cancel
        ));
        assert!(serde_json::from_str::<ClientMsg>(r#"{"type":"drop"}"#).is_err());
    }
}
