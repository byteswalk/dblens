//! TLS 的实库测试（设置 DBLENS_TEST_MYSQL_URL / DBLENS_TEST_MARIADB_URL 时运行）。测试库使用服务器自动生成的自签名证书。

use std::time::Duration;

use dblens_core::{Error, Value};
use dblens_driver::{ConnectParams, Connection, Driver, ExecEvent, collect_all};
use dblens_driver_mysql::MySqlDriver;

fn params(env: &str, tls: Option<&str>) -> Option<ConnectParams> {
    let url = std::env::var(env).ok()?;
    let (_, mut p) = ConnectParams::from_url(&url).unwrap();
    if let Some(t) = tls {
        p.options.insert("tls".into(), t.into());
    }
    Some(p)
}

/// 当前连接的加密套件；未加密时为空串。
async fn cipher(conn: &mut Box<dyn Connection>) -> String {
    let mut stream = conn.execute("SHOW SESSION STATUS LIKE 'Ssl_cipher'", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 10).await.unwrap();
    let v = events
        .into_iter()
        .find_map(|e| if let ExecEvent::Rows(rows) = e { Some(rows[0][1].clone()) } else { None });
    match v {
        Some(Value::String(s)) => s,
        Some(Value::Bytes(b)) => String::from_utf8(b).unwrap(),
        other => panic!("{other:?}"),
    }
}

async fn check(env: &str) {
    let Some(plain) = params(env, None) else { return };
    let mut conn = MySqlDriver.connect(&plain).await.unwrap();
    assert_eq!(cipher(&mut conn).await, "", "默认不加密");
    let mut conn = MySqlDriver.connect(&params(env, Some("require")).unwrap()).await.unwrap();
    assert!(!cipher(&mut conn).await.is_empty(), "require 时应加密");

    // 自签名证书在 verify 模式下应校验失败
    assert!(MySqlDriver.connect(&params(env, Some("verify")).unwrap()).await.is_err());

    // 取消要另开一个连接执行 KILL QUERY，这个连接也走 TLS
    let cancel = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel().await
    });
    let slow = "SELECT COUNT(*) FROM information_schema.columns a, information_schema.columns b, information_schema.columns c";
    let result = async {
        let mut stream = conn.execute(slow, &[]).await?;
        collect_all(stream.as_mut(), 10).await
    };
    let result = tokio::time::timeout(Duration::from_secs(8), result).await.expect("取消后应在 8 秒内返回");
    canceller.await.unwrap().unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
}

#[tokio::test]
async fn mysql_tls() {
    check("DBLENS_TEST_MYSQL_URL").await;
}

#[tokio::test]
async fn mariadb_tls() {
    check("DBLENS_TEST_MARIADB_URL").await;
}
