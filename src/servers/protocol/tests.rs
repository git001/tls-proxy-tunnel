use super::*;
use crate::config::{SniTarget, ViaUpstream};
use crate::upstreams::ProxyToUpstream;
use crate::upstreams::{MetricsEntry, Upstream};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Semaphore;
use tokio_util::task::TaskTracker;

// Real TLS ClientHello for www.lirui.tech — same bytes used in tls.rs tests.
const TLS_CLIENT_HELLO: &[u8] = &[
    0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc, 0x03, 0x03, 0x35, 0x7a, 0xba, 0x3d, 0x89,
    0xd2, 0x5e, 0x7a, 0xa2, 0xd4, 0xe5, 0x6d, 0xd5, 0xa3, 0x98, 0x41, 0xb0, 0xae, 0x41, 0xfc, 0xe6,
    0x64, 0xfd, 0xae, 0x0b, 0x27, 0x6d, 0x90, 0xa8, 0x0a, 0xfa, 0x90, 0x20, 0x59, 0x6f, 0x13, 0x18,
    0x4a, 0xd1, 0x1c, 0xc4, 0x83, 0x8c, 0xfc, 0x93, 0xac, 0x6b, 0x3b, 0xac, 0x67, 0xd0, 0x36, 0xb0,
    0xa2, 0x1b, 0x04, 0xf7, 0xde, 0x02, 0xfb, 0x96, 0x1e, 0xdc, 0x76, 0xa8, 0x00, 0x20, 0x2a, 0x2a,
    0x13, 0x01, 0x13, 0x02, 0x13, 0x03, 0xc0, 0x2b, 0xc0, 0x2f, 0xc0, 0x2c, 0xc0, 0x30, 0xcc, 0xa9,
    0xcc, 0xa8, 0xc0, 0x13, 0xc0, 0x14, 0x00, 0x9c, 0x00, 0x9d, 0x00, 0x2f, 0x00, 0x35, 0x01, 0x00,
    0x01, 0x93, 0xea, 0xea, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13, 0x00, 0x11, 0x00, 0x00, 0x0e, 0x77,
    0x77, 0x77, 0x2e, 0x6c, 0x69, 0x72, 0x75, 0x69, 0x2e, 0x74, 0x65, 0x63, 0x68, 0x00, 0x17, 0x00,
    0x00, 0xff, 0x01, 0x00, 0x01, 0x00, 0x00, 0x0a, 0x00, 0x0a, 0x00, 0x08, 0xba, 0xba, 0x00, 0x1d,
    0x00, 0x17, 0x00, 0x18, 0x00, 0x0b, 0x00, 0x02, 0x01, 0x00, 0x00, 0x23, 0x00, 0x00, 0x00, 0x10,
    0x00, 0x0e, 0x00, 0x0c, 0x02, 0x68, 0x32, 0x08, 0x68, 0x74, 0x74, 0x70, 0x2f, 0x31, 0x2e, 0x31,
    0x00, 0x05, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x12, 0x00, 0x10, 0x04,
    0x03, 0x08, 0x04, 0x04, 0x01, 0x05, 0x03, 0x08, 0x05, 0x05, 0x01, 0x08, 0x06, 0x06, 0x01, 0x00,
    0x12, 0x00, 0x00, 0x00, 0x33, 0x00, 0x2b, 0x00, 0x29, 0xba, 0xba, 0x00, 0x01, 0x00, 0x00, 0x1d,
    0x00, 0x20, 0x3b, 0x45, 0xf9, 0xbc, 0x6e, 0x23, 0x86, 0x41, 0xa5, 0xb2, 0xf5, 0x03, 0xec, 0x67,
    0x4a, 0xd7, 0x9a, 0x17, 0x9f, 0x0c, 0x38, 0x6d, 0x36, 0xf3, 0x4e, 0x5d, 0xa4, 0x7d, 0x15, 0x79,
    0xa4, 0x3f, 0x00, 0x2d, 0x00, 0x02, 0x01, 0x01, 0x00, 0x2b, 0x00, 0x0b, 0x0a, 0xba, 0xba, 0x03,
    0x04, 0x03, 0x03, 0x03, 0x02, 0x03, 0x01, 0x00, 0x1b, 0x00, 0x03, 0x02, 0x00, 0x02, 0x44, 0x69,
    0x00, 0x05, 0x00, 0x03, 0x02, 0x68, 0x32, 0xda, 0xda, 0x00, 0x01, 0x00, 0x00, 0x15, 0x00, 0xc5,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn make_proxy(
    tls: bool,
    default_action: &str,
    upstream: HashMap<String, Upstream>,
    sni: Option<HashMap<String, SniTarget>>,
) -> Arc<Proxy> {
    Arc::new(Proxy {
        name: "test".to_string(),
        listen: "127.0.0.1:0".parse().unwrap(),
        protocol: "tcp".to_string(),
        tls,
        sni,
        default_action: default_action.to_string(),
        upstream: Arc::new(upstream),
        via: ViaUpstream::default(),
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    })
}

// Covers: default upstream not in map → Err("not found")
#[tokio::test]
async fn test_accept_missing_default_upstream() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();

    let proxy = make_proxy(false, "nonexistent", HashMap::new(), None);
    let result = accept(server, proxy).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

// Covers: tls=true + sni=None → "no sni_map" branch
#[tokio::test]
async fn test_accept_tls_no_sni_map_falls_back_to_default() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c.write_all(TLS_CLIENT_HELLO).await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = make_proxy(true, "ban", upstream, None);
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: is_health=true branch in accept()
#[tokio::test]
async fn test_accept_health_server() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c
                .write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let metrics = Arc::new(vec![]);
    let mut upstream = HashMap::new();
    upstream.insert("health".to_string(), Upstream::Health(metrics));
    let proxy = make_proxy(false, "health", upstream, None);
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: token.cancelled() branch in proxy()
#[tokio::test]
async fn test_proxy_loop_shutdown() {
    let tmp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let listen_addr = tmp.local_addr().unwrap();
    drop(tmp);

    let token = CancellationToken::new();
    token.cancel();

    let tracker = TaskTracker::new();
    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let p = Arc::new(Proxy {
        name: "test".to_string(),
        listen: listen_addr,
        protocol: "tcp".to_string(),
        tls: false,
        sni: None,
        default_action: "ban".to_string(),
        upstream: Arc::new(upstream),
        via: ViaUpstream::default(),
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    });

    let result = proxy(p, token, tracker).await;
    assert!(result.is_ok());
}

// Covers: use_sni_as_target=true → connect_target derived from SNI
#[tokio::test]
async fn test_accept_use_sni_as_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c.write_all(TLS_CLIENT_HELLO).await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let via = ViaUpstream {
        use_sni_as_target: true,
        ..Default::default()
    };

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = Arc::new(Proxy {
        name: "test".to_string(),
        listen: "127.0.0.1:0".parse().unwrap(),
        protocol: "tcp".to_string(),
        tls: true,
        sni: None,
        default_action: "ban".to_string(),
        upstream: Arc::new(upstream),
        via,
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    });

    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: use_sni_as_target=false + non-empty target → static connect_target
#[tokio::test]
async fn test_accept_static_connect_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();

    let via = ViaUpstream {
        target: "example.com:443".to_string(),
        ..Default::default()
    };

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = Arc::new(Proxy {
        name: "test".to_string(),
        listen: "127.0.0.1:0".parse().unwrap(),
        protocol: "tcp".to_string(),
        tls: false,
        sni: None,
        default_action: "ban".to_string(),
        upstream: Arc::new(upstream),
        via,
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    });

    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: SNI found in map with Extended SniTarget + via_override
#[tokio::test]
async fn test_accept_tls_sni_extended_target() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c.write_all(TLS_CLIENT_HELLO).await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let mut sni_map: HashMap<String, SniTarget> = HashMap::new();
    sni_map.insert(
        "www.lirui.tech".to_string(),
        SniTarget::Extended {
            upstream: "ban".to_string(),
            via: Some(ViaUpstream::default()),
        },
    );

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = make_proxy(true, "ban", upstream, Some(sni_map));
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: SNI present but not in map → fallback to default
#[tokio::test]
async fn test_accept_tls_sni_not_in_map() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c.write_all(TLS_CLIENT_HELLO).await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let mut sni_map: HashMap<String, SniTarget> = HashMap::new();
    sni_map.insert(
        "other.example.com".to_string(),
        SniTarget::Simple("ban".to_string()),
    );

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = make_proxy(true, "ban", upstream, Some(sni_map));
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: SNI found in map as Simple target → via_override = None
#[tokio::test]
async fn test_accept_tls_sni_simple_match() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok(mut c) = TcpStream::connect(addr).await {
            let _ = c.write_all(TLS_CLIENT_HELLO).await;
        }
    });
    let (server, _) = listener.accept().await.unwrap();

    let mut sni_map: HashMap<String, SniTarget> = HashMap::new();
    sni_map.insert(
        "www.lirui.tech".to_string(),
        SniTarget::Simple("ban".to_string()),
    );

    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let proxy = make_proxy(true, "ban", upstream, Some(sni_map));
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: accept() result Err arm — error log (lines 187–190)
// Upstream::Proxy to a refused port → process() returns Err → logged, accept() still Ok
#[tokio::test]
async fn test_accept_upstream_process_error_logged() {
    let refused = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let refused_addr = refused.local_addr().unwrap();
    drop(refused); // port is now free → connections will be refused

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _client = TcpStream::connect(addr).await.unwrap();
    let (server, _) = listener.accept().await.unwrap();

    let proxy_upstream = ProxyToUpstream::new(refused_addr.to_string(), "tcp".to_string());
    let mut upstream = HashMap::new();
    upstream.insert("proxy".to_string(), Upstream::Proxy(proxy_upstream));
    let proxy = make_proxy(false, "proxy", upstream, None);

    // accept() always returns Ok — the Err from process() is only logged
    let result = accept(server, proxy).await;
    assert!(result.is_ok());
}

// Covers: proxy() non-health tracker.spawn path (lines 74–79)
// tokio::select! avoids the Send requirement that tokio::spawn would impose.
#[tokio::test]
async fn test_proxy_non_health_dispatch() {
    let tmp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tmp.local_addr().unwrap();
    drop(tmp);

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();
    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let p = Arc::new(Proxy {
        name: "test".to_string(),
        listen: addr,
        protocol: "tcp".to_string(),
        tls: false,
        sni: None,
        default_action: "ban".to_string(),
        upstream: Arc::new(upstream),
        via: ViaUpstream::default(),
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    });

    let token_clone = token.clone();
    tokio::select! {
        result = proxy(p, token_clone, tracker) => { result.unwrap(); }
        _ = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = TcpStream::connect(addr).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            token.cancel();
            tokio::time::sleep(Duration::from_millis(10)).await;
        } => {}
    }
}

// Covers: proxy() health server tracker.spawn path (lines 48–56)
#[tokio::test]
async fn test_proxy_health_dispatch() {
    let tmp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tmp.local_addr().unwrap();
    drop(tmp);

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();
    let metrics = Arc::new(vec![MetricsEntry {
        name: "test".to_string(),
        listen: addr.to_string(),
        maxclients_limit: 10,
        semaphore: Arc::new(Semaphore::new(10)),
    }]);
    let mut upstream = HashMap::new();
    upstream.insert("health".to_string(), Upstream::Health(metrics));
    let p = Arc::new(Proxy {
        name: "test".to_string(),
        listen: addr,
        protocol: "tcp".to_string(),
        tls: false,
        sni: None,
        default_action: "health".to_string(),
        upstream: Arc::new(upstream),
        via: ViaUpstream::default(),
        maxclients: Arc::new(Semaphore::new(10)),
        maxclients_limit: 10,
    });

    let token_clone = token.clone();
    tokio::select! {
        result = proxy(p, token_clone, tracker) => { result.unwrap(); }
        _ = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = TcpStream::connect(addr).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            token.cancel();
            tokio::time::sleep(Duration::from_millis(10)).await;
        } => {}
    }
}

// Covers: proxy() maxclients exceeded → warn + drop connection (lines 62–72)
#[tokio::test]
async fn test_proxy_maxclients_exceeded() {
    let tmp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tmp.local_addr().unwrap();
    drop(tmp);

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();
    let mut upstream = HashMap::new();
    upstream.insert("ban".to_string(), Upstream::Ban);
    let p = Arc::new(Proxy {
        name: "test".to_string(),
        listen: addr,
        protocol: "tcp".to_string(),
        tls: false,
        sni: None,
        default_action: "ban".to_string(),
        upstream: Arc::new(upstream),
        via: ViaUpstream::default(),
        maxclients: Arc::new(Semaphore::new(0)), // no permits → all connections rejected
        maxclients_limit: 0,
    });

    let token_clone = token.clone();
    tokio::select! {
        result = proxy(p, token_clone, tracker) => { result.unwrap(); }
        _ = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = TcpStream::connect(addr).await;
            tokio::time::sleep(Duration::from_millis(20)).await;
            token.cancel();
            tokio::time::sleep(Duration::from_millis(10)).await;
        } => {}
    }
}
