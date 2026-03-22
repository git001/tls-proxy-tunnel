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

#[derive(Debug, Default, Deserialize, Clone)]
pub struct ViaUpstream {
    #[serde(default)]
    pub headers: Arc<HashMap<String, String>>,
    /// Static CONNECT target (host:port). Ignored when `use_sni_as_target` is true.
    #[serde(default)]
    pub target: String,
    #[serde(default = "default_connect_timeout", with = "humantime_serde")]
    pub connect_timeout: Duration,
    /// Derive the CONNECT target dynamically from the TLS SNI instead of `target`.
    #[serde(default)]
    pub use_sni_as_target: bool,
    /// Port appended to the SNI hostname when `use_sni_as_target` is true.
    #[serde(default = "default_target_port")]
    pub target_port: u16,
    /// How often to log in-flight rx/tx byte counters. `Duration::ZERO` = disabled.
    #[serde(default, with = "humantime_serde")]
    pub stats_interval: Duration,
}

pub(super) fn default_connect_timeout() -> Duration {
    Duration::from_secs(30)
}

fn default_target_port() -> u16 {
    443
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
}

pub(super) fn default_maxclients() -> usize {
    100
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
    fn test_default_maxclients() {
        assert_eq!(default_maxclients(), 100);
    }
}
