use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::upstreams::Upstream;

// ---------------------------------------------------------------------------
// Top-level config wrappers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Config {
    pub base: ParsedConfig,
}

/// Config after YAML parsing + upstream resolution.
#[derive(Debug, Default, Clone)]
pub struct ParsedConfig {
    pub version: i32,
    pub log: Option<String>,
    pub servers: HashMap<String, ServerConfig>,
    pub upstream: HashMap<String, Upstream>,
}

/// Raw YAML representation — deserialized directly from the config file.
#[derive(Debug, Default, Deserialize, Clone)]
pub struct BaseConfig {
    pub version: i32,
    pub log: Option<String>,
    #[serde(rename = "log-format")]
    pub log_format: Option<String>,
    pub servers: HashMap<String, ServerConfig>,
    #[serde(default)]
    pub upstream: HashMap<String, String>,
    /// Top-level `via:` block used as a YAML anchor target only — not read in code.
    #[serde(default)]
    #[allow(dead_code)]
    pub via: ViaUpstream,
}

// ---------------------------------------------------------------------------
// ViaUpstream — HTTP CONNECT proxy settings
// ---------------------------------------------------------------------------

/// Missing fields take their value from `Default` below — also when the
/// whole `via:` block is missing, so both paths agree (they did not: a
/// server without `via:` used to get connect_timeout 0 and target_port 0).
#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct ViaUpstream {
    pub headers: Arc<HashMap<String, String>>,
    /// Static CONNECT target (host:port). Ignored when `use_sni_as_target` is true.
    pub target: String,
    /// Deadline for DNS lookup + TCP connect + CONNECT response.
    /// `Duration::ZERO` = no limit.
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,
    /// Derive the CONNECT target dynamically from the TLS SNI instead of `target`.
    pub use_sni_as_target: bool,
    /// Port appended to the SNI hostname when `use_sni_as_target` is true.
    pub target_port: u16,
    /// How often to log in-flight rx/tx byte counters. `Duration::ZERO` = disabled.
    #[serde(with = "humantime_serde")]
    pub stats_interval: Duration,
    /// Close a relayed connection after no data flowed in either direction
    /// for this long. `Duration::ZERO` = disabled (the default).
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,
}

impl Default for ViaUpstream {
    fn default() -> Self {
        Self {
            headers: Arc::default(),
            target: String::new(),
            connect_timeout: default_connect_timeout(),
            use_sni_as_target: false,
            target_port: 443,
            stats_interval: Duration::ZERO,
            idle_timeout: Duration::ZERO,
        }
    }
}

pub(super) fn default_connect_timeout() -> Duration {
    Duration::from_secs(30)
}

// ---------------------------------------------------------------------------
// SniTarget — per-SNI routing entry
// ---------------------------------------------------------------------------

/// Per-SNI routing target.
///
/// ```yaml
/// sni:
///   intern.corp.org: direct_upstream           # plain string
///   extern.corp.org:
///     upstream: corp_proxy
///     via:
///       use_sni_as_target: true
///       target_port: 443
/// ```
#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum SniTarget {
    /// Just an upstream name — inherits server-level `via`.
    Simple(String),
    /// Upstream name plus an optional per-SNI `via` override.
    Extended {
        upstream: String,
        #[serde(default)]
        via: Option<ViaUpstream>,
    },
}

impl SniTarget {
    pub fn upstream_name(&self) -> &str {
        match self {
            SniTarget::Simple(name) => name,
            SniTarget::Extended { upstream, .. } => upstream,
        }
    }

    pub fn via_override(&self) -> Option<&ViaUpstream> {
        match self {
            SniTarget::Simple(_) => None,
            SniTarget::Extended { via, .. } => via.as_ref(),
        }
    }
}

// ---------------------------------------------------------------------------
// ServerConfig
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ServerConfig {
    pub listen: Vec<String>,
    pub protocol: Option<String>,
    pub tls: Option<bool>,
    pub sni: Option<HashMap<String, SniTarget>>,
    pub default: Option<String>,
    #[serde(default)]
    pub via: ViaUpstream,
    #[serde(default = "default_maxclients")]
    pub maxclients: usize,
    /// TLS servers: how long a client may take to send its ClientHello.
    /// Bounds slow or silent clients that would hold a maxclients slot.
    /// `Duration::ZERO` = no limit.
    #[serde(default = "default_client_hello_timeout", with = "humantime_serde")]
    pub client_hello_timeout: Duration,
}

pub(super) fn default_maxclients() -> usize {
    100
}

pub(super) fn default_client_hello_timeout() -> Duration {
    Duration::from_secs(10)
}

// ---------------------------------------------------------------------------
// Tests for default-value functions
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_connect_timeout() {
        assert_eq!(default_connect_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_via_default_matches_missing_fields() {
        let empty: ViaUpstream = serde_saphyr::from_str("{}").unwrap();
        let default = ViaUpstream::default();
        assert_eq!(empty.connect_timeout, default.connect_timeout);
        assert_eq!(empty.target_port, 443);
        assert_eq!(default.connect_timeout, Duration::from_secs(30));
    }

    #[test]
    fn test_default_maxclients() {
        assert_eq!(default_maxclients(), 100);
    }

    #[test]
    fn test_default_client_hello_timeout() {
        assert_eq!(default_client_hello_timeout(), Duration::from_secs(10));
    }
}
