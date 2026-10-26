//! 浏览器模式的服务入口（设计文档 4.1 节）。
//!
//! ```text
//! dblens-server [--listen 127.0.0.1:8742] [--data-dir 目录] [--web-dir 目录] [--allow-origin 来源]...
//!               [--update-url 地址] [--no-update-check]
//! ```
//!
//! 启动时生成一次访问令牌并打印出来，本次运行期间有效。首期只供单用户使用，多用户鉴权留给团队版。
//!
//! 前端页面默认取可执行文件旁边的 `web/` 目录；找不到时只提供接口，并在日志中提示。
//!
//! 启动后在后台检查一次新版本（默认读 GitHub Releases 的 `latest.json`，`--update-url` 可改成内网地址），
//! 有新版本时在日志中提示，不会自动替换自己。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use dblens_api::{ApiConfig, new_token, router, serve, with_web};
use dblens_core::msg;
use dblens_service::{Service, default_data_dir};

mod update;

const DEFAULT_LISTEN: &str = "127.0.0.1:8742";

#[derive(Debug, PartialEq, Eq)]
struct Args {
    listen: SocketAddr,
    data_dir: Option<PathBuf>,
    web_dir: Option<PathBuf>,
    allowed_origins: Vec<String>,
    /// 更新源；`None` 表示不检查新版本。
    update_url: Option<String>,
}

fn usage() -> String {
    msg!("server.usage").current()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Args, String> {
    let mut out = Args {
        listen: DEFAULT_LISTEN.parse().unwrap(),
        data_dir: None,
        web_dir: None,
        allowed_origins: Vec::new(),
        update_url: Some(update::DEFAULT_URL.to_string()),
    };
    let mut no_update_check = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .ok_or_else(|| format!("{}\n{}", msg!("server.missing_value", arg = name).current(), usage()))
        };
        match a.as_str() {
            "--listen" => {
                let v = value("--listen")?;
                out.listen = v.parse().map_err(|_| msg!("server.bad_listen", value = v).current())?;
            }
            "--data-dir" => out.data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--web-dir" => out.web_dir = Some(PathBuf::from(value("--web-dir")?)),
            "--allow-origin" => out.allowed_origins.push(value("--allow-origin")?),
            "--update-url" => out.update_url = Some(value("--update-url")?),
            "--no-update-check" => no_update_check = true,
            "-h" | "--help" => return Err(usage()),
            other => {
                return Err(format!("{}\n{}", msg!("server.unknown_arg", arg = other).current(), usage()));
            }
        }
    }
    if no_update_check {
        out.update_url = None;
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
        eprintln!("{}", msg!("server.start_failed", error = msg).current());
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
        .map_err(|e| msg!("server.bind_failed", addr = args.listen, error = e).current())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    if !addr.ip().is_loopback() {
        tracing::warn!("正在监听非本机地址 {addr}：同一网络中的其他人只要拿到访问令牌就能访问");
    }
    let web_dir = resolve_web_dir(args.web_dir, std::env::current_exe().ok());
    // 第一行是地址、第二行是令牌，便于脚本读取
    println!("{}", msg!("server.started", url = format!("http://{addr}")).current());
    println!("{}", msg!("server.token", token = token).current());
    println!("{}", msg!("server.data_dir", dir = data_dir.display()).current());
    // 令牌放在 # 后面：浏览器不会把它发给服务器，也不会出现在访问日志和 Referer 中
    match &web_dir {
        Some(_) => {
            println!("{}", msg!("server.open_url", url = format!("http://{addr}/#token={token}")).current())
        }
        None => tracing::warn!("没有找到前端页面目录（可用 --web-dir 指定），只提供接口"),
    }
    if let Some(url) = args.update_url {
        tokio::spawn(async move {
            match update::check(&url, env!("CARGO_PKG_VERSION")).await {
                Ok(Some(text)) => tracing::info!("{text}"),
                Ok(None) => tracing::info!("已是最新版本"),
                Err(e) => tracing::warn!("检查新版本失败：{e}"),
            }
        });
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
    use dblens_core::{Lang, with_lang};

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
        assert_eq!(a.update_url.as_deref(), Some(update::DEFAULT_URL));
        assert_eq!(
            parse(&["--update-url", "http://intranet/latest.json"]).unwrap().update_url.as_deref(),
            Some("http://intranet/latest.json")
        );
        assert_eq!(parse(&["--update-url", "http://x", "--no-update-check"]).unwrap().update_url, None);
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
        let zh = |args: &[&str]| with_lang(Lang::ZhCn, || parse(args).unwrap_err());
        assert!(zh(&["--listen"]).contains("缺少参数"));
        assert!(zh(&["--listen", "nope"]).contains("无效"));
        assert!(zh(&["--bogus"]).contains("未知参数"));
        assert!(with_lang(Lang::En, || parse(&["--bogus"]).unwrap_err()).contains("Unknown argument"));
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
