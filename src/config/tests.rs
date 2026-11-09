use super::*;
use crate::config::SniTarget;
use std::time::Duration;

#[test]
fn test_load_config() {
    let config = Config::new("tests/config.yaml").unwrap();
    assert_eq!(config.base.version, 2);
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

// tpt 4 reads only version 2; a version 1 config is rejected with a hint.
#[test]
fn test_load_config_version_1_rejected() {
    let err = Config::new("tests/config_version_1.yaml").unwrap_err();
    assert!(err.to_string().contains("needs 'version: 2'"), "{err}");
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
    assert_eq!(config.base.version, 2);
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

    // Every server and via field set
    let full = config.base.servers.get("full_config_server").unwrap();
    assert_eq!(full.maxclients, 200);
    assert_eq!(full.client_hello_timeout, Duration::from_secs(5));
    assert_eq!(full.via.connect_timeout, Duration::from_secs(60));
    assert_eq!(full.via.stats_interval, Duration::from_secs(30));
    assert_eq!(full.via.idle_timeout, Duration::from_secs(300));
    assert_eq!(full.via.headers.len(), 3);

    // Defaults when nothing is set
    let minimal = config.base.servers.get("minimal_server").unwrap();
    assert_eq!(minimal.maxclients, 100);
    assert_eq!(minimal.client_hello_timeout, Duration::from_secs(10));
    assert_eq!(minimal.via.connect_timeout, Duration::from_secs(30));
    assert_eq!(minimal.via.idle_timeout, Duration::ZERO);
    assert_eq!(minimal.via.stats_interval, Duration::ZERO);
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

// The RUST_LOG tests change the process environment; tests run in parallel
// threads, so they take this lock (set_var is unsafe for that reason).
static RUST_LOG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// Covers: logger init block (non-disable log level), txt/default_format branch,
// RUST_LOG not set → parse_filters called
#[test]
fn test_load_config_log_txt() {
    let _guard = RUST_LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::remove_var("RUST_LOG") };
    let result = Config::new("tests/config_log_txt.yaml");
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// Covers: RUST_LOG set → parse_filters skipped
#[test]
fn test_load_config_rust_log_set() {
    let _guard = RUST_LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("RUST_LOG", "warn") };
    let result = Config::new("tests/config_log_txt.yaml");
    unsafe { std::env::remove_var("RUST_LOG") };
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
}

// Covers: log_format = json → json formatter branch
#[test]
fn test_load_config_log_json() {
    let _guard = RUST_LOG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

// --- unknown keys are errors ---

/// Load YAML text through the normal path (a temp file + Config::new).
fn load_str(yaml: &str) -> Result<Config, ConfigError> {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), yaml).unwrap();
    Config::new(file.path().to_str().unwrap())
}

const MINIMAL: &str =
    "version: 2\nlog: disable\nservers:\n  s:\n    listen: [\"127.0.0.1:1\"]\n    default: echo\n";

#[test]
fn test_unknown_key_top_level() {
    let err = load_str(&format!("{MINIMAL}shutdown_timout: 5s\n")).unwrap_err();
    assert!(err.to_string().contains("shutdown_timout"), "{err}");
}

#[test]
fn test_unknown_key_server() {
    let err = load_str(&format!("{MINIMAL}    client_hello_timout: 5s\n")).unwrap_err();
    assert!(err.to_string().contains("client_hello_timout"), "{err}");
}

#[test]
fn test_unknown_key_via() {
    let err = load_str(&format!("{MINIMAL}    via:\n      idle_timout: 5s\n")).unwrap_err();
    assert!(err.to_string().contains("idle_timout"), "{err}");
}

#[test]
fn test_unknown_key_sni_entry() {
    let yaml = format!("{MINIMAL}    tls: true\n    sni:\n      a.test:\n        upstrem: echo\n");
    let err = load_str(&yaml).unwrap_err();
    assert!(err.to_string().contains("upstrem"), "{err}");
}

#[test]
fn test_sni_entry_forms() {
    let yaml = format!(
        "{MINIMAL}    tls: true\n    sni:\n      a.test: echo\n      b.test:\n        upstream: echo\n        via: {{}}\n"
    );
    let config = load_str(&yaml).unwrap();
    let sni = config.base.servers["s"].sni.as_ref().unwrap();
    assert!(matches!(&sni["a.test"], SniTarget::Simple(n) if n == "echo"));
    assert_eq!(sni["b.test"].upstream_name(), "echo");
    assert_eq!(
        sni["b.test"].via_override().unwrap().connect_timeout,
        Duration::from_secs(30)
    );
}

#[test]
fn test_shutdown_timeout() {
    assert_eq!(
        load_str(MINIMAL).unwrap().base.shutdown_timeout,
        Duration::from_secs(25)
    );
    let set = load_str(&format!("{MINIMAL}shutdown_timeout: 3s\n")).unwrap();
    assert_eq!(set.base.shutdown_timeout, Duration::from_secs(3));
}

// Every config shipped in the repo loads — examples must match the code.
#[test]
fn test_shipped_configs_load() {
    for path in [
        "config.yaml.example",
        "example-config.yaml",
        "container-files/etc/tpt/config.yaml",
        "tests/config.yaml",
        "tests/config_full.yaml",
    ] {
        if let Err(e) = Config::new(path) {
            panic!("{path}: {e}");
        }
    }
    // The OpenShift ConfigMap carries the config as a string.
    let cm: std::collections::HashMap<String, serde_json::Value> = serde_saphyr::from_str(
        &std::fs::read_to_string("openshift/01-tpt-k8-config.yaml").unwrap(),
    )
    .unwrap();
    let embedded = cm["data"]["config.yaml"].as_str().unwrap();
    if let Err(e) = load_str(embedded) {
        panic!("openshift/01-tpt-k8-config.yaml: {e}");
    }
}
