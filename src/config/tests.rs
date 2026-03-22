use super::*;

#[test]
fn test_load_config() {
    let config = Config::new("tests/config.yaml").unwrap();
    assert_eq!(config.base.version, 1);
    assert_eq!(config.base.log.unwrap(), "disable");
    assert_eq!(config.base.servers.len(), 4);
    assert_eq!(config.base.upstream.len(), 3 + 3);
}

#[test]
fn test_try_from_valid_tcp() {
    let result = ProxyToUpstream::try_from("tcp://example.com:80");
    assert!(result.is_ok());
    let ups = result.unwrap();
    assert_eq!(ups.addr, "example.com:80");
    assert_eq!(ups.protocol, "tcp");
}

#[test]
fn test_try_from_valid_tcp4() {
    let ups = ProxyToUpstream::try_from("tcp4://127.0.0.1:8080").unwrap();
    assert_eq!(ups.protocol, "tcp4");
}

#[test]
fn test_try_from_valid_tcp6() {
    let ups = ProxyToUpstream::try_from("tcp6://[::1]:9000").unwrap();
    assert_eq!(ups.protocol, "tcp6");
}

#[test]
fn test_try_from_invalid_url() {
    assert!(matches!(
        ProxyToUpstream::try_from("not-a-url"),
        Err(ConfigError::Custom(_))
    ));
}

#[test]
fn test_try_from_invalid_scheme() {
    assert!(matches!(
        ProxyToUpstream::try_from("http://example.com:80"),
        Err(ConfigError::Custom(_))
    ));
}

#[test]
fn test_try_from_no_host() {
    assert!(matches!(
        ProxyToUpstream::try_from("tcp:///path"),
        Err(ConfigError::Custom(_))
    ));
}

#[test]
fn test_load_config_version_mismatch() {
    assert!(matches!(
        Config::new("tests/config_bad_version.yaml"),
        Err(ConfigError::Custom(_))
    ));
}

#[test]
fn test_load_config_not_found() {
    assert!(matches!(
        Config::new("tests/nonexistent.yaml"),
        Err(ConfigError::IO(_))
    ));
}

#[test]
fn test_load_config_bad_yaml() {
    assert!(matches!(
        Config::new("tests/config_bad_yaml.yaml"),
        Err(ConfigError::Yaml(_))
    ));
}

#[test]
fn test_load_config_full() {
    let config = Config::new("tests/config_full.yaml").unwrap();
    assert_eq!(config.base.version, 1);
    assert_eq!(config.base.servers.len(), 15);
    assert_eq!(config.base.upstream.len(), 5 + 3);

    let tls_plain = config.base.servers.get("tls_plain_sni_server").unwrap();
    let sni_map = tls_plain.sni.as_ref().unwrap();
    assert_eq!(
        sni_map.get("www.example.com").unwrap().upstream_name(),
        "web_server"
    );
    assert!(
        sni_map
            .get("www.example.com")
            .unwrap()
            .via_override()
            .is_none()
    );

    let mixed = config
        .base
        .servers
        .get("tls_extended_direct_override_server")
        .unwrap();
    let intern = mixed.sni.as_ref().unwrap().get("intern.corp.org").unwrap();
    assert_eq!(intern.upstream_name(), "direct_host");
    let intern_via = intern.via_override().unwrap();
    assert!(!intern_via.use_sni_as_target);
    assert!(intern_via.target.is_empty());

    let full = config
        .base
        .servers
        .get("tls_mixed_strategies_server")
        .unwrap();
    let d = full.sni.as_ref().unwrap().get("d.example.com").unwrap();
    let d_via = d.via_override().unwrap();
    assert!(d_via.use_sni_as_target);
    assert_eq!(d_via.target_port, 8443);
}

#[test]
fn test_duplicate_listen_address_rejected() {
    let result = Config::new("tests/config_duplicate_listen.yaml");
    assert!(
        matches!(result, Err(ConfigError::Custom(ref m)) if m.contains("Duplicate listen address")),
        "expected duplicate-listen error, got: {:?}",
        result
    );
}

#[test]
fn test_missing_upstream_rejected() {
    let result = Config::new("tests/config_missing_upstream.yaml");
    assert!(
        matches!(result, Err(ConfigError::Custom(ref m)) if m.contains("Upstream") && m.contains("not found")),
        "expected missing-upstream error, got: {:?}",
        result
    );
}

#[test]
fn test_unused_upstream_is_allowed() {
    let result = Config::new("tests/config_unused_upstream.yaml");
    assert!(
        result.is_ok(),
        "unused upstream should not fail: {:?}",
        result
    );
}

#[test]
fn test_load_config_bad_log_format() {
    let result = Config::new("tests/config_bad_log_format.yaml");
    assert!(
        matches!(result, Err(ConfigError::Custom(ref m)) if m.contains("log-format")),
        "expected bad-log-format error, got: {:?}",
        result
    );
}

// Covers: logger init block (non-disable log level), txt/default_format branch,
// RUST_LOG not set → parse_filters called
#[test]
fn test_load_config_log_txt() {
    unsafe { std::env::remove_var("RUST_LOG") };
    let result = Config::new("tests/config_log_txt.yaml");
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// Covers: RUST_LOG set → parse_filters skipped
#[test]
fn test_load_config_rust_log_set() {
    unsafe { std::env::set_var("RUST_LOG", "warn") };
    let result = Config::new("tests/config_log_txt.yaml");
    unsafe { std::env::remove_var("RUST_LOG") };
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// Covers: log_format = json → json formatter branch
#[test]
fn test_load_config_log_json() {
    unsafe { std::env::remove_var("RUST_LOG") };
    let result = Config::new("tests/config_log_json.yaml");
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// Covers: TryFrom URL with no port → port_or_known_default() returns None
#[test]
fn test_try_from_no_port() {
    assert!(matches!(
        ProxyToUpstream::try_from("tcp://example.com"),
        Err(ConfigError::Custom(_))
    ));
}
