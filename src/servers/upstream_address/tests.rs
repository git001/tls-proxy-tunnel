use super::*;
use time::Duration;

// --- ResolutionMode ---

#[test]
fn test_resolution_mode_tcp4() {
    assert!(matches!(ResolutionMode::from("tcp4"), ResolutionMode::Ipv4));
}

#[test]
fn test_resolution_mode_tcp6() {
    assert!(matches!(ResolutionMode::from("tcp6"), ResolutionMode::Ipv6));
}

#[test]
fn test_resolution_mode_tcp() {
    assert!(matches!(
        ResolutionMode::from("tcp"),
        ResolutionMode::Ipv4AndIpv6
    ));
}

#[test]
fn test_resolution_mode_unknown_falls_back() {
    assert!(matches!(
        ResolutionMode::from("udp"),
        ResolutionMode::Ipv4AndIpv6
    ));
}

#[test]
fn test_resolution_mode_display() {
    assert_eq!(ResolutionMode::Ipv4.to_string(), "IPv4Only");
    assert_eq!(ResolutionMode::Ipv6.to_string(), "IPv6Only");
    assert_eq!(ResolutionMode::Ipv4AndIpv6.to_string(), "IPv4 and IPv6");
}

#[test]
fn test_resolution_mode_default() {
    assert!(matches!(
        ResolutionMode::default(),
        ResolutionMode::Ipv4AndIpv6
    ));
}

// --- UpstreamAddress ---

#[tokio::test]
async fn test_is_valid_initial() {
    let addr = UpstreamAddress::new("example.com:80".to_string());
    assert!(!addr.is_valid().await);
}

#[tokio::test]
async fn test_is_resolved_initial() {
    let addr = UpstreamAddress::new("example.com:80".to_string());
    assert!(!addr.is_resolved().await);
}

#[tokio::test]
async fn test_time_remaining_unresolved() {
    let addr = UpstreamAddress::new("example.com:80".to_string());
    assert_eq!(addr.time_remaining().await, Duration::seconds(0));
}

#[tokio::test]
async fn test_display() {
    let addr = UpstreamAddress::new("myhost:1234".to_string());
    assert_eq!(addr.to_string(), "myhost:1234");
}

#[tokio::test]
async fn test_resolve_localhost() {
    let addr = UpstreamAddress::new("127.0.0.1:80".to_string());
    let result = addr.resolve(ResolutionMode::Ipv4AndIpv6).await;
    assert!(result.is_ok());
    let addrs = result.unwrap();
    assert!(!addrs.is_empty());
    assert!(addr.is_valid().await);
    assert!(addr.is_resolved().await);
    assert!(addr.time_remaining().await > Duration::seconds(0));
}

#[tokio::test]
async fn test_resolve_caches_result() {
    let addr = UpstreamAddress::new("127.0.0.1:80".to_string());
    let first = addr.resolve(ResolutionMode::Ipv4AndIpv6).await.unwrap();
    let second = addr.resolve(ResolutionMode::Ipv4AndIpv6).await.unwrap();
    assert_eq!(first, second);
}

#[tokio::test]
async fn test_resolve_invalid_host_sets_error_ttl() {
    let addr = UpstreamAddress::new("this.host.does.not.exist.invalid:80".to_string());
    let result = addr.resolve(ResolutionMode::Ipv4AndIpv6).await;
    assert!(result.is_err());
    // After a failed lookup the error TTL (3s) is set and is_valid() should be false
    // because resolved_addresses is still empty (is_resolved() is false)
    assert!(!addr.is_resolved().await);
}

#[tokio::test]
async fn test_resolve_ipv4_filter() {
    let addr = UpstreamAddress::new("127.0.0.1:80".to_string());
    let addrs = addr.resolve(ResolutionMode::Ipv4).await.unwrap();
    for a in &addrs {
        assert!(a.is_ipv4());
    }
}

#[tokio::test]
async fn test_resolve_ipv6_filter() {
    // [::1] is the IPv6 loopback — always available on Linux.
    // The IPv6 filter must keep only IPv6 addresses.
    let addr = UpstreamAddress::new("[::1]:80".to_string());
    let result = addr.resolve(ResolutionMode::Ipv6).await;
    match result {
        Ok(addrs) => {
            assert!(
                !addrs.is_empty(),
                "expected at least one IPv6 address for [::1]"
            );
            for a in &addrs {
                assert!(a.is_ipv6(), "expected IPv6, got {:?}", a);
            }
        }
        Err(e) => panic!("resolve([::1]) failed: {}", e),
    }
}
