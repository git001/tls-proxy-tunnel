use super::*;

// --- parse_connect_status ---

#[test]
fn test_parse_connect_status_valid() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 200 Connection established\r\n\r\n").unwrap(),
        200
    );
}

#[test]
fn test_parse_connect_status_201() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 201 Created\r\n\r\n").unwrap(),
        201
    );
}

#[test]
fn test_parse_connect_status_407() {
    assert_eq!(
        parse_connect_status(b"HTTP/1.1 407 Proxy Authentication Required\r\n").unwrap(),
        407
    );
}

#[test]
fn test_parse_connect_status_no_whitespace() {
    assert!(parse_connect_status(b"GARBAGE").is_err());
}

#[test]
fn test_parse_connect_status_nonnumeric_status() {
    assert!(parse_connect_status(b"HTTP/1.1 abc OK\r\n").is_err());
}

// --- resolve_header_value ---

#[test]
fn test_resolve_header_value_no_dollar() {
    let result = resolve_header_value("Basic abc123").unwrap();
    assert_eq!(result, "Basic abc123");
}

#[test]
fn test_resolve_header_value_lone_dollar() {
    let result = resolve_header_value("$").unwrap();
    assert_eq!(result, "$");
}

#[test]
fn test_resolve_header_value_with_var() {
    unsafe { std::env::set_var("TPT_TEST_RESOLVE_VAR", "secret") };
    let result = resolve_header_value("Bearer $TPT_TEST_RESOLVE_VAR").unwrap();
    assert_eq!(result, "Bearer secret");
}

#[test]
fn test_resolve_header_value_missing_var() {
    unsafe { std::env::remove_var("TPT_TEST_MISSING_VAR") };
    assert!(resolve_header_value("$TPT_TEST_MISSING_VAR").is_err());
}

// --- http_connect: upstream misbehaves ---

/// A proxy on 127.0.0.1 that reads the CONNECT request and then runs `reply`.
async fn connect_against<F, Fut>(reply: F) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(TcpStream) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut req = Vec::new();
        let mut buf = [0u8; 1024];
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = sock.read(&mut buf).await.unwrap();
            assert!(n > 0, "client closed before sending CONNECT");
            req.extend_from_slice(&buf[..n]);
        }
        reply(sock).await;
    });
    let outbound = TcpStream::connect(addr).await.unwrap();
    let result = http_connect(&outbound, "example.org:443", &HashMap::new()).await;
    drop(outbound); // lets a server that waits for EOF finish
    server.await.unwrap();
    result
}

#[tokio::test]
async fn test_http_connect_proxy_closes_before_response() {
    let err = connect_against(|sock| async move { drop(sock) })
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("closed connection before sending CONNECT response"),
        "{err}"
    );
}

#[tokio::test]
async fn test_http_connect_response_header_too_large() {
    let err = connect_against(|mut sock| async move {
        use tokio::io::AsyncWriteExt;
        // More than the 16 KiB read buffer, never a \r\n\r\n.
        let _ = sock.write_all(&[b'x'; 20000]).await;
        // Keep the socket open until the client gives up.
        let mut buf = [0u8; 16];
        let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
    })
    .await
    .unwrap_err();
    assert!(
        err.to_string()
            .contains("CONNECT response header too large"),
        "{err}"
    );
}

#[tokio::test]
async fn test_http_connect_2xx_ok_and_non_2xx_err() {
    use tokio::io::AsyncWriteExt;
    connect_against(|mut sock| async move {
        sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await
            .unwrap();
    })
    .await
    .unwrap();
    assert!(
        connect_against(|mut sock| async move {
            sock.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n")
                .await
                .unwrap();
        })
        .await
        .is_err()
    );
}
