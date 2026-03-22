use super::*;
use crate::config::ViaUpstream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;

// A reader that immediately returns an error — triggers copy()'s Err branch.
struct BrokenReader;
impl AsyncRead for BrokenReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "test")))
    }
}
impl Unpin for BrokenReader {}

// A writer that discards all bytes.
struct NullWriter;
impl AsyncWrite for NullWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl Unpin for NullWriter {}

// Covers: copy() Ok branch
#[tokio::test]
async fn test_copy_success() {
    let (mut write_end, mut read_end) = tokio::io::duplex(1024);
    write_end.write_all(b"hello").await.unwrap();
    drop(write_end);
    let result = copy(&mut read_end, &mut NullWriter).await;
    assert_eq!(result.unwrap(), 5);
}

// Covers: copy() Err branch → returns Ok(0) after shutdown
#[tokio::test]
async fn test_copy_error_branch() {
    let result = copy(&mut BrokenReader, &mut NullWriter).await;
    assert_eq!(result.unwrap(), 0);
}

// Covers: Upstream::Ban in process()
#[tokio::test]
async fn test_upstream_ban() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();
    let result = Upstream::Ban
        .process(server, &ViaUpstream::default(), None)
        .await;
    assert!(result.is_ok());
}

// Covers: Upstream::Echo in process() + copy() with real data
#[tokio::test]
async fn test_upstream_echo() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client_task = tokio::spawn(async move {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        client.shutdown().await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        buf
    });
    let (server, _) = listener.accept().await.unwrap();
    Upstream::Echo
        .process(server, &ViaUpstream::default(), None)
        .await
        .unwrap();
    assert_eq!(client_task.await.unwrap(), b"ping");
}

// Covers: health_handler() default path → "OK"
#[tokio::test]
async fn test_upstream_health_ok_path() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client_task = tokio::spawn(async move {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        String::from_utf8_lossy(&resp).to_string()
    });
    let (server, _) = listener.accept().await.unwrap();
    Upstream::Health(Arc::new(vec![]))
        .process(server, &ViaUpstream::default(), None)
        .await
        .unwrap();
    let resp = client_task.await.unwrap();
    assert!(resp.contains("200"));
    assert!(resp.contains("OK"));
}

// Covers: health_handler() /metrics path → Prometheus format
#[tokio::test]
async fn test_upstream_health_metrics_path() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client_task = tokio::spawn(async move {
        let mut client = TcpStream::connect(addr).await.unwrap();
        client
            .write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        client.read_to_end(&mut resp).await.unwrap();
        String::from_utf8_lossy(&resp).to_string()
    });
    let (server, _) = listener.accept().await.unwrap();
    let metrics = Arc::new(vec![MetricsEntry {
        name: "test_server".to_string(),
        listen: "127.0.0.1:9999".to_string(),
        maxclients_limit: 50,
        semaphore: Arc::new(Semaphore::new(45)),
    }]);
    Upstream::Health(metrics)
        .process(server, &ViaUpstream::default(), None)
        .await
        .unwrap();
    let resp = client_task.await.unwrap();
    assert!(resp.contains("tpt_active_connections"));
    assert!(resp.contains("tpt_maxclients"));
    assert!(resp.contains("test_server"));
    assert!(resp.contains("text/plain; version=0.0.4"));
}
