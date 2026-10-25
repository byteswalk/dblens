//! TLS 的实库测试（设置 DBLENS_TEST_PG_URL 时运行）。测试库使用自签名证书。

use std::time::Duration;

use dblens_core::{Error, Value};
use dblens_driver::{ConnectParams, Connection, Driver, ExecEvent, collect_all};
use dblens_driver_postgres::PgDriver;

fn params(tls: Option<&str>) -> Option<ConnectParams> {
    let url = std::env::var("DBLENS_TEST_PG_URL").ok()?;
    let (_, mut p) = ConnectParams::from_url(&url).unwrap();
    if let Some(t) = tls {
        p.options.insert("tls".into(), t.into());
    }
    Some(p)
}

async fn encrypted(conn: &mut Box<dyn Connection>) -> Value {
    let mut stream =
        conn.execute("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()", &[]).await.unwrap();
    let events = collect_all(stream.as_mut(), 10).await.unwrap();
    events
        .into_iter()
        .find_map(|e| if let ExecEvent::Rows(rows) = e { Some(rows[0][0].clone()) } else { None })
        .unwrap()
}

#[tokio::test]
async fn require_encrypts_and_default_does_not() {
    let Some(plain) = params(None) else { return };
    let mut conn = PgDriver.connect(&plain).await.unwrap();
    assert_eq!(encrypted(&mut conn).await, Value::Bool(false));
    let mut conn = PgDriver.connect(&params(Some("require")).unwrap()).await.unwrap();
    assert_eq!(encrypted(&mut conn).await, Value::Bool(true));
}

#[tokio::test]
async fn verify_rejects_self_signed_certificate() {
    let Some(p) = params(Some("verify")) else { return };
    let err = PgDriver.connect(&p).await.err().expect("自签名证书应校验失败");
    assert!(matches!(err, Error::Connect { .. }), "{err:?}");
}

#[tokio::test]
async fn cancel_works_over_tls() {
    let Some(p) = params(Some("require")) else { return };
    let mut conn = PgDriver.connect(&p).await.unwrap();
    let cancel = conn.cancel_handle();
    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        cancel.cancel().await
    });
    let result = async {
        let mut stream = conn.execute("SELECT pg_sleep(20)", &[]).await?;
        collect_all(stream.as_mut(), 10).await
    };
    let result = tokio::time::timeout(Duration::from_secs(8), result).await.expect("取消后应在 8 秒内返回");
    canceller.await.unwrap().unwrap();
    assert!(matches!(result, Err(Error::Cancelled)), "{result:?}");
}
