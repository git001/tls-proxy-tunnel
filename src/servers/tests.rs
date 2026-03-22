use std::net::SocketAddr;
use std::thread::{self, sleep};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

#[tokio::main]
async fn tcp_mock_server() {
    let server_addr: SocketAddr = "127.0.0.1:54599".parse().unwrap();
    let listener = TcpListener::bind(server_addr).await.unwrap();
    loop {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 2];
        let mut n = stream.read(&mut buf).await.unwrap();
        while n > 0 {
            let _ = stream.write(b"hello").await.unwrap();
            if buf.eq(b"by") {
                stream.shutdown().await.unwrap();
                break;
            }
            n = stream.read(&mut buf).await.unwrap();
        }
        stream.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn test_proxy() {
    use crate::config::Config;
    let config = Config::new("tests/config.yaml").unwrap();
    let mut server = Server::from(config.base);
    thread::spawn(move || {
        tcp_mock_server();
    });
    sleep(Duration::from_secs(1)); // wait for server to start
    thread::spawn(move || {
        let _ = server.run();
    });
    sleep(Duration::from_secs(1)); // wait for server to start

    // test TCP proxy
    let mut conn = tokio::net::TcpStream::connect("127.0.0.1:54500")
        .await
        .unwrap();
    let mut buf = [0u8; 5];
    let _ = conn.write(b"hi").await.unwrap();
    let _ = conn.read(&mut buf).await.unwrap();
    assert_eq!(&buf, b"hello");
    conn.shutdown().await.unwrap();

    // test TCP echo
    let mut conn = tokio::net::TcpStream::connect("127.0.0.1:54956")
        .await
        .unwrap();
    let mut buf = [0u8; 1];
    for i in 0..=10u8 {
        let _ = conn.write(&[i]).await.unwrap();
        let _ = conn.read(&mut buf).await.unwrap();
        assert_eq!(&buf, &[i]);
    }
    conn.shutdown().await.unwrap();
}
