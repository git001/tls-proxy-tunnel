use super::*;
use socket2::SockRef;
use tokio::net::TcpListener;

#[tokio::test]
async fn test_set_keepalive() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    assert!(!SockRef::from(&client).keepalive().unwrap());
    set_keepalive(&client);
    let sock = SockRef::from(&client);
    assert!(sock.keepalive().unwrap());
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        assert_eq!(sock.tcp_keepalive_time().unwrap(), Duration::from_secs(60));
        assert_eq!(
            sock.tcp_keepalive_interval().unwrap(),
            Duration::from_secs(10)
        );
        assert_eq!(sock.tcp_keepalive_retries().unwrap(), 3);
    }
}
