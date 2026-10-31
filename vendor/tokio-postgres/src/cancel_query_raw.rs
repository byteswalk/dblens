use crate::config::{SslMode, SslNegotiation};
use crate::tls::TlsConnect;
use crate::{Error, connect_tls};
use bytes::BytesMut;
use postgres_protocol::message::frontend;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn cancel_query_raw<S, T>(
    stream: S,
    mode: SslMode,
    negotiation: SslNegotiation,
    tls: T,
    has_hostname: bool,
    process_id: i32,
    secret_key: i32,
) -> Result<(), Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
    T: TlsConnect<S>,
{
    let mut stream = connect_tls::connect_tls(stream, mode, negotiation, tls, has_hostname).await?;

    let mut buf = BytesMut::new();
    frontend::cancel_request(process_id, secret_key, &mut buf);

    stream.write_all(&buf).await.map_err(Error::io)?;
    stream.flush().await.map_err(Error::io)?;
    // 和 libpq 一样等服务器先关连接：抢先关闭时，PolarDB 这类代理会丢掉还没转发的取消请求
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut [0u8; 1])).await;
    stream.shutdown().await.map_err(Error::io)?;

    Ok(())
}
