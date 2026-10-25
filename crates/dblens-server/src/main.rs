//! 浏览器模式的服务入口（设计文档 4.1 节）。
//!
//! ```text
//! dblens-server [--listen 127.0.0.1:8742] [--data-dir 目录] [--allow-origin 来源]...
//! ```
//!
//! 启动时生成一次访问令牌并打印出来，本次运行期间有效。首期只供单用户使用，多用户鉴权留给团队版。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use dblens_api::{ApiConfig, new_token, router, serve};
use dblens_service::{Service, default_data_dir};

const DEFAULT_LISTEN: &str = "127.0.0.1:8742";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    listen: SocketAddr,
    data_dir: Option<PathBuf>,
    allowed_origins: Vec<String>,
}

const USAGE: &str = "用法：dblens-server [--listen 地址:端口] [--data-dir 目录] [--allow-origin 来源]...";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out =
        Args { listen: DEFAULT_LISTEN.parse().unwrap(), data_dir: None, allowed_origins: Vec::new() };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} 后面缺少参数\n{USAGE}"));
        match a.as_str() {
            "--listen" => {
                let v = value("--listen")?;
                out.listen = v.parse().map_err(|_| format!("无效的监听地址：{v}"))?;
            }
            "--data-dir" => out.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--allow-origin" => out.allowed_origins.push(value("--allow-origin")?),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("未知参数：{other}\n{USAGE}")),
        }
    }
    Ok(out)
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(msg) = run(args).await {
        eprintln!("启动失败：{msg}");
        std::process::exit(1);
    }
}

async fn run(args: Args) -> Result<(), String> {
    let data_dir = match args.data_dir {
        Some(d) => d,
        None => default_data_dir().map_err(|e| e.to_string())?,
    };
    let service = Arc::new(Service::open(&data_dir).map_err(|e| e.to_string())?);
    let token = new_token();
    let listener = tokio::net::TcpListener::bind(args.listen)
        .await
        .map_err(|e| format!("无法监听 {}：{e}", args.listen))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    if !addr.ip().is_loopback() {
        tracing::warn!("正在监听非本机地址 {addr}：同一网络中的其他人只要拿到访问令牌就能访问");
    }
    // 第一行是地址、第二行是令牌，便于脚本读取
    println!("DBLens 服务已启动：http://{addr}");
    println!("访问令牌：{token}");
    println!("数据目录：{}", data_dir.display());
    let app = router(service, ApiConfig { token, allowed_origins: args.allowed_origins });
    serve(listener, app).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_arguments() {
        let a = parse(&[]).unwrap();
        assert_eq!(
            (a.listen.to_string(), a.data_dir, a.allowed_origins.len()),
            ("127.0.0.1:8742".into(), None, 0)
        );
        let a = parse(&[
            "--listen",
            "0.0.0.0:9000",
            "--data-dir",
            "d",
            "--allow-origin",
            "http://x",
            "--allow-origin",
            "http://y",
        ])
        .unwrap();
        assert_eq!(a.listen.to_string(), "0.0.0.0:9000");
        assert_eq!(a.data_dir, Some(PathBuf::from("d")));
        assert_eq!(a.allowed_origins, ["http://x", "http://y"]);
        assert!(parse(&["--listen"]).unwrap_err().contains("缺少参数"));
        assert!(parse(&["--listen", "nope"]).unwrap_err().contains("无效"));
        assert!(parse(&["--bogus"]).unwrap_err().contains("未知参数"));
    }
}
