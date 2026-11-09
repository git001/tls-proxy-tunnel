use super::*;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpListener;

// Reader that immediately returns a BrokenPipe error.
struct ErrReader;
impl AsyncRead for ErrReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "test")))
    }
}
impl Unpin for ErrReader {}

// Writer that always fails on write_all.
struct ErrWriter;
impl AsyncWrite for ErrWriter {
    fn poll_write(self: Pin<&mut Self>, _: &mut Context, _buf: &[u8]) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "test")))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
impl Unpin for ErrWriter {}

// Covers: copy_counted() normal data flow + counter update
#[tokio::test]
async fn test_copy_counted_ok() {
    let (mut write_end, mut read_end) = tokio::io::duplex(1024);
    write_end.write_all(b"hello world").await.unwrap();
    drop(write_end);

    let counter = Arc::new(AtomicU64::new(0));
    let (mut sink_write, _sink_read) = tokio::io::duplex(1024);

    let n = copy_counted(&mut read_end, &mut sink_write, &counter, &Activity::new())
        .await
        .unwrap();
    assert_eq!(n, 11);
    assert_eq!(counter.load(Ordering::Relaxed), 11);
}

// Covers: copy_counted() read error branch
#[tokio::test]
async fn test_copy_counted_read_error() {
    let counter = Arc::new(AtomicU64::new(0));
    let (mut sink_write, _sink_read) = tokio::io::duplex(1024);

    let n = copy_counted(&mut ErrReader, &mut sink_write, &counter, &Activity::new())
        .await
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(counter.load(Ordering::Relaxed), 0);
}

// Covers: copy_counted() write error branch
#[tokio::test]
async fn test_copy_counted_write_error() {
    let (mut write_end, mut read_end) = tokio::io::duplex(1024);
    write_end.write_all(b"data").await.unwrap();
    drop(write_end);

    let counter = Arc::new(AtomicU64::new(0));
    let n = copy_counted(&mut read_end, &mut ErrWriter, &counter, &Activity::new())
        .await
        .unwrap();
    assert_eq!(n, 0);
}

// Covers: relay() without stats_interval (log_handle = None)
#[tokio::test]
async fn test_relay_no_stats() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        client.shutdown().await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        buf
    });

    let (inbound, _) = listener.accept().await.unwrap();

    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo_listener.local_addr().unwrap();
    let echo_task = tokio::spawn(async move {
        let (mut conn, _) = echo_listener.accept().await.unwrap();
        let (mut r, mut w) = io::split(&mut conn);
        io::copy(&mut r, &mut w).await.unwrap();
    });

    let outbound = tokio::net::TcpStream::connect(echo_addr).await.unwrap();
    let (tx, rx) = relay(
        inbound,
        outbound,
        "test".to_string(),
        Duration::ZERO,
        Duration::ZERO,
    )
    .await
    .unwrap();

    let echoed = client_task.await.unwrap();
    let _ = echo_task.await;

    assert_eq!(tx, 4); // "ping"
    assert_eq!(rx, 4); // echoed back
    assert_eq!(echoed, b"ping");
}

// Covers: relay() with stats_interval > 0 (log_handle spawned + aborted)
#[tokio::test]
async fn test_relay_with_stats_interval() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let client_task = tokio::spawn(async move {
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.shutdown().await.unwrap();
    });

    let (inbound, _) = listener.accept().await.unwrap();

    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo_listener.local_addr().unwrap();
    let echo_task = tokio::spawn(async move {
        let (mut conn, _) = echo_listener.accept().await.unwrap();
        let (mut r, mut w) = io::split(&mut conn);
        io::copy(&mut r, &mut w).await.unwrap();
    });

    let outbound = tokio::net::TcpStream::connect(echo_addr).await.unwrap();
    // Long interval — we only need to cover the spawn + abort path, not the log line.
    let (tx, rx) = relay(
        inbound,
        outbound,
        "stats_test".to_string(),
        Duration::from_secs(3600),
        Duration::ZERO,
    )
    .await
    .unwrap();

    client_task.await.unwrap();
    let _ = echo_task.await;

    assert_eq!(tx, 0);
    assert_eq!(rx, 0);
}

// Covers: relay() idle timeout — nothing flows, the relay ends by itself
// and both connections close.
#[tokio::test]
async fn test_relay_idle_timeout_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client_task = tokio::spawn(async move {
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        // Send once, then stay silent: the relay must close us.
        client.write_all(b"x").await.unwrap();
        let mut buf = Vec::new();
        client.read_to_end(&mut buf).await.unwrap();
        buf
    });
    let (inbound, _) = listener.accept().await.unwrap();

    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo_listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut conn, _) = echo_listener.accept().await.unwrap();
        let (mut r, mut w) = io::split(&mut conn);
        let _ = io::copy(&mut r, &mut w).await;
    });
    let outbound = tokio::net::TcpStream::connect(echo_addr).await.unwrap();

    let started = std::time::Instant::now();
    let (tx, rx) = tokio::time::timeout(
        Duration::from_secs(5),
        relay(
            inbound,
            outbound,
            "idle_test".to_string(),
            Duration::ZERO,
            Duration::from_millis(300),
        ),
    )
    .await
    .expect("relay did not end on idle timeout")
    .unwrap();
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!((tx, rx), (1, 1));
    assert_eq!(client_task.await.unwrap(), b"x");
}

// Covers: Activity — reads reset the idle clock
#[tokio::test]
async fn test_activity_touch_resets_idle() {
    let a = Activity::new();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(a.idle_for() >= Duration::from_millis(50));
    a.touch();
    assert!(a.idle_for() < Duration::from_millis(50));
}
