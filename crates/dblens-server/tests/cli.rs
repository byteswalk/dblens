//! 启动真实的 dblens-server 进程，读出它打印的地址和令牌，再用 HTTP 请求验证令牌校验。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

/// 发一个最简单的 HTTP/1.1 GET 请求，返回状态码。
fn get_status(addr: &str, path: &str, token: Option<&str>) -> u16 {
    let mut stream = TcpStream::connect(addr).unwrap();
    let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
    write!(stream, "GET {path} HTTP/1.1\r\nHost: {addr}\r\n{auth}Connection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn prints_address_and_token_and_enforces_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dblens-server"))
        .args(["--listen", "127.0.0.1:0", "--no-update-check", "--data-dir"])
        .env("DBLENS_LANG", "zh-CN")
        .arg(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let first = lines.next().unwrap().unwrap();
    let second = lines.next().unwrap().unwrap();
    let addr = first.rsplit("http://").next().unwrap().trim().to_string();
    let token = second.split('：').nth(1).unwrap().trim().to_string();
    assert_eq!(token.len(), 64, "{second}");

    assert_eq!(get_status(&addr, "/api/profiles", None), 401);
    assert_eq!(get_status(&addr, "/api/profiles", Some("wrong")), 401);
    assert_eq!(get_status(&addr, "/api/profiles", Some(&token)), 200);
    assert!(dir.path().join("dblens.db").is_file(), "本地库建在指定的数据目录中");
    child.kill().unwrap();
    let _ = child.wait();
}

#[test]
fn serves_web_dir_and_prints_open_link() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("web");
    std::fs::create_dir(&web).unwrap();
    std::fs::write(web.join("index.html"), "<html></html>").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dblens-server"))
        .args(["--listen", "127.0.0.1:0", "--no-update-check", "--data-dir"])
        .env("DBLENS_LANG", "zh-CN")
        .arg(dir.path().join("data"))
        .arg("--web-dir")
        .arg(&web)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines: Vec<String> =
        BufReader::new(child.stdout.take().unwrap()).lines().take(4).map(Result::unwrap).collect();
    let addr = lines[0].rsplit("http://").next().unwrap().trim().to_string();
    let token = lines[1].split('：').nth(1).unwrap().trim().to_string();
    assert_eq!(lines[3], format!("在浏览器中打开：http://{addr}/#token={token}"));

    assert_eq!(get_status(&addr, "/", None), 200, "页面不需要令牌");
    assert_eq!(get_status(&addr, "/api/profiles", None), 401, "接口仍然需要令牌");
    child.kill().unwrap();
    let _ = child.wait();
}

/// 在本机起一个只应答一次的 HTTP 服务，返回 `latest.json`。
fn serve_latest_json_once(body: &'static str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        write!(
            s,
            "HTTP/1.1 200 OK
Content-Type: application/json
Content-Length: {}
Connection: close

{body}",
            body.len()
        )
        .unwrap();
    });
    format!("http://{addr}/latest.json")
}

#[test]
fn reports_newer_version_from_update_url() {
    let url = serve_latest_json_once(r#"{"version":"99.0.0","notes":"新功能","platforms":{}}"#);
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dblens-server"))
        .args(["--listen", "127.0.0.1:0", "--update-url", &url, "--data-dir"])
        .env("DBLENS_LANG", "zh-CN")
        .arg(dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let line = BufReader::new(child.stderr.take().unwrap())
        .lines()
        .map(Result::unwrap)
        .find(|l| l.contains("发现新版本") || l.contains("检查新版本失败"))
        .unwrap();
    child.kill().unwrap();
    let _ = child.wait();
    assert!(line.contains("发现新版本 99.0.0（当前 0.1.0）"), "{line}");
}
