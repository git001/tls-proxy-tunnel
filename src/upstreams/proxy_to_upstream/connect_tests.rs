use super::*;
use crate::servers::upstream_address::UpstreamAddress;

#[tokio::test]
async fn test_unknown_protocol_returns_err() {
    let addr = UpstreamAddress::new("127.0.0.1:12345".to_string());
    let result = connect_upstream(&addr, "udp").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown protocol"));
}
