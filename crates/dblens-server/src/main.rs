//! 浏览器模式的服务入口（设计文档 4.1 节）。
//!
//! ```text
//! dblens-server [--listen 127.0.0.1:8742] [--data-dir 目录] [--web-dir 目录] [--allow-origin 来源]...
//! ```
//!
//! 启动时生成一次访问令牌并打印出来，本次运行期间有效。首期只供单用户使用，多用户鉴权留给团队版。
//!
//! 前端页面默认取可执行文件旁边的 `web/` 目录；找不到时只提供接口，并在日志中提示。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use dblens_api::{ApiConfig, new_token, router, serve, with_web};
use dblens_service::{Service, default_data_dir};

const DEFAULT_LISTEN: &str = "127.0.0.1:8742";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    listen: SocketAddr,
    data_dir: Option<PathBuf>,
    web_dir: Option<PathBuf>,
    allowed_origins: Vec<String>,
}

const USAGE: &str =
    "用法：dblens-server [--listen 地址:端口] [--data-dir 目录] [--web-dir 目录] [--allow-origin 来源]...";

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        listen: DEFAULT_LISTEN.parse().unwrap(),
        data_dir: None,
        web_dir: None,
        allowed_origins: Vec::new(),
    };
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} 后面缺少参数\n{USAGE}"));
        match a.as_str() {
            "--listen" => {
                let v = value("--listen")?;
                out.listen = v.parse().map_err(|_| format!("无效的监听地址：{v}"))?;
            }
            "--data-dir" => out.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--web-dir" => out.web_dir = Some(PathBuf::from(value("--web-dir")?)),
            "--allow-origin" => out.allowed_origins.push(value("--allow-origin")?),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("未知参数：{other}\n{USAGE}")),
        }
    }
    Ok(out)
}

/// 前端页面目录：命令行指定的优先，否则取可执行文件旁边的 `web/`。目录里没有 `index.html` 时返回 `None`。
fn resolve_web_dir(arg: Option<PathBuf>, exe: Option<PathBuf>) -> Option<PathBuf> {
    let dir = arg.or_else(|| exe?.parent().map(|p| p.join("web")))?;
    dir.join("index.html").is_file().then_some(dir)
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
    let web_dir = resolve_web_dir(args.web_dir, std::env::current_exe().ok());
    // 第一行是地址、第二行是令牌，便于脚本读取
    println!("DBLens 服务已启动：http://{addr}");
    println!("访问令牌：{token}");
    println!("数据目录：{}", data_dir.display());
    // 令牌放在 # 后面：浏览器不会把它发给服务器，也不会出现在访问日志和 Referer 中
    match &web_dir {
        Some(_) => println!("在浏览器中打开：http://{addr}/#token={token}"),
        None => tracing::warn!("没有找到前端页面目录（可用 --web-dir 指定），只提供接口"),
    }
    let mut app = router(service, ApiConfig { token, allowed_origins: args.allowed_origins });
    if let Some(dir) = web_dir {
        app = with_web(app, dir);
    }
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
            (a.listen.to_string(), a.data_dir, a.web_dir, a.allowed_origins.len()),
            ("127.0.0.1:8742".into(), None, None, 0)
        );
        let a = parse(&[
            "--listen",
            "0.0.0.0:9000",
            "--data-dir",
            "d",
            "--web-dir",
            "w",
            "--allow-origin",
            "http://x",
            "--allow-origin",
            "http://y",
        ])
        .unwrap();
        assert_eq!(a.listen.to_string(), "0.0.0.0:9000");
        assert_eq!(a.data_dir, Some(PathBuf::from("d")));
        assert_eq!(a.web_dir, Some(PathBuf::from("w")));
        assert_eq!(a.allowed_origins, ["http://x", "http://y"]);
        assert!(parse(&["--listen"]).unwrap_err().contains("缺少参数"));
        assert!(parse(&["--listen", "nope"]).unwrap_err().contains("无效"));
        assert!(parse(&["--bogus"]).unwrap_err().contains("未知参数"));
    }

    #[test]
    fn web_dir_needs_index_html() {
        let tmp = tempfile::tempdir().unwrap();
        let web = tmp.path().join("web");
        std::fs::create_dir(&web).unwrap();
        let exe = Some(tmp.path().join("dblens-server.exe"));
        assert_eq!(resolve_web_dir(None, exe.clone()), None, "目录里还没有 index.html");
        std::fs::write(web.join("index.html"), "x").unwrap();
        assert_eq!(resolve_web_dir(None, exe.clone()), Some(web.clone()), "默认取可执行文件旁边的 web/");
        let other = tmp.path().join("other");
        assert_eq!(resolve_web_dir(Some(other), exe), None, "命令行指定的目录优先，即使它不可用");
        assert_eq!(resolve_web_dir(None, None), None);
    }
}
