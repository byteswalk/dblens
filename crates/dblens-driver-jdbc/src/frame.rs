//! Java 桥接进程的线上格式。Java 端见 bridge/src/dblens/bridge/Frame.java，两边必须同时改。

use std::io::{self, Read, Write};

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use dblens_core::{Error, LobPreview, Result, Row, Value, msg};

/// 单帧上限：防止协议错位时按乱码长度分配内存。
const MAX_FRAME: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum Req {
    Connect {
        driver_class: String,
        url: String,
        user: String,
        password: String,
        jar_dir: String,
        timeout_secs: u32,
    },
    Execute {
        conn: u64,
        sql: String,
        params: Vec<Value>,
    },
    Fetch {
        conn: u64,
        max_rows: u32,
    },
    Cancel {
        conn: u64,
    },
    Commit {
        conn: u64,
    },
    Rollback {
        conn: u64,
    },
    SetAutocommit {
        conn: u64,
        on: bool,
    },
    Ping {
        conn: u64,
    },
    Close {
        conn: u64,
    },
    ServerInfo {
        conn: u64,
    },
    SetLobLimit {
        conn: u64,
        bytes: u64,
    },
    EndExec {
        conn: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
    pub nullable: Option<bool>,
    pub precision: Option<u32>,
    pub scale: Option<i32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Resp {
    Ok,
    Connected(u64),
    Result(Vec<Column>),
    Affected(u64),
    Rows { done: bool, rows: Vec<Row> },
    Error { code: i32, sqlstate: String, message: String },
    ServerInfo { product: String, version: String },
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_be_bytes());
    out.extend(s.as_bytes());
}

fn put_value(out: &mut Vec<u8>, v: &Value) -> Result<()> {
    match v {
        Value::Null => out.push(0),
        Value::Bool(b) => out.extend([1, u8::from(*b)]),
        Value::Int(i) => {
            out.push(2);
            out.extend(i.to_be_bytes());
        }
        Value::Float(f) => {
            out.push(3);
            out.extend(f.to_be_bytes());
        }
        Value::Decimal(s) => {
            out.push(4);
            put_str(out, s);
        }
        Value::String(s) => {
            out.push(5);
            put_str(out, s);
        }
        Value::Bytes(b) => {
            out.push(6);
            out.extend((b.len() as u32).to_be_bytes());
            out.extend(b);
        }
        Value::Date(d) => {
            out.push(7);
            put_str(out, &d.format("%Y-%m-%d").to_string());
        }
        Value::Time(t) => {
            out.push(8);
            put_str(out, &t.format("%H:%M:%S%.6f").to_string());
        }
        Value::DateTime(t) => {
            out.push(9);
            put_str(out, &t.format("%Y-%m-%dT%H:%M:%S%.6f").to_string());
        }
        other => return Err(Error::invalid(msg!("jdbc.unsupported_param", value = format!("{other:?}")))),
    }
    Ok(())
}

/// 编码一条请求，含帧头。
pub fn encode_req(id: u64, req: &Req) -> Result<Vec<u8>> {
    let mut b = Vec::new();
    let (kind, conn) = match req {
        Req::Connect { driver_class, url, user, password, jar_dir, timeout_secs } => {
            for s in [driver_class, url, user, password, jar_dir] {
                put_str(&mut b, s);
            }
            b.extend(timeout_secs.to_be_bytes());
            (1u8, None)
        }
        Req::Execute { conn, sql, params } => {
            b.extend(conn.to_be_bytes());
            put_str(&mut b, sql);
            b.extend((params.len() as u32).to_be_bytes());
            for p in params {
                put_value(&mut b, p)?;
            }
            (2, None)
        }
        Req::Fetch { conn, max_rows } => {
            b.extend(conn.to_be_bytes());
            b.extend(max_rows.to_be_bytes());
            (3, None)
        }
        Req::Cancel { conn } => (4, Some(*conn)),
        Req::Commit { conn } => (5, Some(*conn)),
        Req::Rollback { conn } => (6, Some(*conn)),
        Req::SetAutocommit { conn, on } => {
            b.extend(conn.to_be_bytes());
            b.push(u8::from(*on));
            (7, None)
        }
        Req::Ping { conn } => (8, Some(*conn)),
        Req::Close { conn } => (9, Some(*conn)),
        Req::ServerInfo { conn } => (10, Some(*conn)),
        Req::SetLobLimit { conn, bytes } => {
            b.extend(conn.to_be_bytes());
            b.extend(bytes.to_be_bytes());
            (11, None)
        }
        Req::EndExec { conn } => (12, Some(*conn)),
    };
    if let Some(c) = conn {
        b.extend(c.to_be_bytes());
    }
    let mut out = Vec::with_capacity(b.len() + 13);
    out.extend(((b.len() + 9) as u32).to_be_bytes());
    out.extend(id.to_be_bytes());
    out.push(kind);
    out.extend(b);
    Ok(out)
}

struct Cur<'a>(&'a [u8]);

impl Cur<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.0.len() < n {
            return Err(Error::connect(msg!("jdbc.bad_frame")));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("长度已检查")))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().expect("长度已检查")))
    }
    fn string(&mut self) -> Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| Error::connect(msg!("jdbc.bad_frame")))
    }
    fn opt_u32(&mut self) -> Result<Option<u32>> {
        Ok(if self.u8()? == 1 { Some(self.u32()?) } else { None })
    }
    fn value(&mut self) -> Result<Value> {
        let bad = || Error::connect(msg!("jdbc.bad_frame"));
        Ok(match self.u8()? {
            0 => Value::Null,
            1 => Value::Bool(self.u8()? != 0),
            2 => Value::Int(self.u64()? as i64),
            3 => Value::Float(f64::from_bits(self.u64()?)),
            4 => Value::Decimal(self.string()?),
            5 => Value::String(self.string()?),
            6 => {
                let n = self.u32()? as usize;
                Value::Bytes(self.take(n)?.to_vec())
            }
            7 => Value::Date(NaiveDate::parse_from_str(&self.string()?, "%Y-%m-%d").map_err(|_| bad())?),
            8 => Value::Time(NaiveTime::parse_from_str(&self.string()?, "%H:%M:%S%.f").map_err(|_| bad())?),
            9 => Value::DateTime(
                NaiveDateTime::parse_from_str(&self.string()?, "%Y-%m-%dT%H:%M:%S%.f").map_err(|_| bad())?,
            ),
            10 => Value::Lob(LobPreview {
                binary: self.u8()? != 0,
                preview: self.string()?,
                total_len: self.u64()?,
                truncated: self.u8()? != 0,
            }),
            11 => Value::Other { type_name: self.string()?, text: self.string()? },
            _ => return Err(bad()),
        })
    }
}

/// 解码响应内容。`columns` 是当前结果集的列数，只有 rows 用到。
pub fn decode_resp(kind: u8, body: &[u8], columns: usize) -> Result<Resp> {
    let mut c = Cur(body);
    Ok(match kind {
        128 => Resp::Ok,
        129 => Resp::Connected(c.u64()?),
        130 => {
            let n = c.u32()? as usize;
            let mut cols = Vec::with_capacity(n);
            for _ in 0..n {
                cols.push(Column {
                    name: c.string()?,
                    type_name: c.string()?,
                    nullable: match c.u8()? {
                        0 => Some(false),
                        1 => Some(true),
                        _ => None,
                    },
                    precision: c.opt_u32()?,
                    scale: c.opt_u32()?.map(|v| v as i32),
                });
            }
            Resp::Result(cols)
        }
        131 => Resp::Affected(c.u64()?),
        132 => {
            let done = c.u8()? != 0;
            let total = c.u32()? as usize;
            let mut values = Vec::with_capacity(total);
            for _ in 0..total {
                values.push(c.value()?);
            }
            let rows = if columns == 0 {
                Vec::new()
            } else {
                values.chunks(columns).map(<[Value]>::to_vec).collect()
            };
            Resp::Rows { done, rows }
        }
        133 => Resp::Error { code: c.u32()? as i32, sqlstate: c.string()?, message: c.string()? },
        134 => Resp::ServerInfo { product: c.string()?, version: c.string()? },
        _ => return Err(Error::connect(msg!("jdbc.bad_frame"))),
    })
}

/// 读一帧，返回（请求号，类型，内容）。
pub fn read_frame(r: &mut impl Read) -> io::Result<(u64, u8, Vec<u8>)> {
    let mut head = [0u8; 13];
    r.read_exact(&mut head)?;
    let len = u32::from_be_bytes(head[..4].try_into().expect("定长")) as usize;
    if !(9..=MAX_FRAME).contains(&len) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "帧长度异常"));
    }
    let mut body = vec![0; len - 9];
    r.read_exact(&mut body)?;
    Ok((u64::from_be_bytes(head[4..12].try_into().expect("定长")), head[12], body))
}

pub fn write_frame(w: &mut impl Write, frame: &[u8]) -> io::Result<()> {
    w.write_all(frame)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dblens_core::Value;

    #[test]
    fn execute_request_matches_the_shared_sample() {
        let bytes =
            encode_req(7, &Req::Execute { conn: 2, sql: "select ?".into(), params: vec![Value::Int(1)] })
                .unwrap();
        // 这组字节同时写在 bridge/test/dblens/bridge/FrameTest.java 里，两端必须一致
        assert_eq!(
            hex(&bytes),
            "0000002a000000000000000702000000000000000200000008\
             73656c656374203f00000001020000000000000001"
        );
    }

    #[test]
    fn rows_response_round_trip() {
        let mut body = vec![0u8, 0, 0, 0, 2, 5];
        body.extend([0, 0, 0, 6]);
        body.extend("数洞".as_bytes());
        body.push(0);
        assert_eq!(
            decode_resp(132, &body, 1).unwrap(),
            Resp::Rows { done: false, rows: vec![vec![Value::String("数洞".into())], vec![Value::Null]] }
        );
    }

    #[test]
    fn truncated_frame_is_an_error_not_a_panic() {
        assert!(decode_resp(133, &[0, 0], 0).is_err());
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }
}
