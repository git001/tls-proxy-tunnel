use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use std::collections::HashMap;
use std::fmt;
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
    /// How long SIGTERM waits for active connections (ZERO = no limit).
    pub shutdown_timeout: Duration,
    pub servers: HashMap<String, ServerConfig>,
    pub upstream: HashMap<String, Upstream>,
}

/// Raw YAML representation — deserialized directly from the config file.
/// Unknown keys are errors (a typo must not be ignored silently), here and
/// in every nested config struct.
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BaseConfig {
    pub version: i32,
    pub log: Option<String>,
    #[serde(rename = "log-format")]
    pub log_format: Option<String>,
    /// On SIGTERM/SIGINT: how long to wait for active connections before
    /// exiting anyway. Below Kubernetes' default grace period of 30s.
    #[serde(default = "default_shutdown_timeout", with = "humantime_serde")]
    pub shutdown_timeout: Duration,
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
#[serde(default, deny_unknown_fields)]
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

pub(super) fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(25)
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
#[derive(Debug, Clone)]
pub enum SniTarget {
    /// Just an upstream name — inherits server-level `via`.
    Simple(String),
    /// Upstream name plus an optional per-SNI `via` override.
    Extended {
        upstream: String,
        via: Option<ViaUpstream>,
    },
}

/// A string is `Simple`, a map is `Extended` (unknown keys rejected).
/// Hand-written instead of `#[serde(untagged)]`, whose only error is "did
/// not match any variant" — this way a typo in an entry is named.
impl<'de> Deserialize<'de> for SniTarget {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Extended {
            upstream: String,
            #[serde(default)]
            via: Option<ViaUpstream>,
        }

        struct SniTargetVisitor;

        impl<'de> Visitor<'de> for SniTargetVisitor {
            type Value = SniTarget;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an upstream name or a map with `upstream` and optional `via`")
            }

            fn visit_str<E: de::Error>(self, name: &str) -> Result<SniTarget, E> {
                Ok(SniTarget::Simple(name.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<SniTarget, A::Error> {
                let e = Extended::deserialize(de::value::MapAccessDeserializer::new(map))?;
                Ok(SniTarget::Extended {
                    upstream: e.upstream,
                    via: e.via,
                })
            }
        }

        deserializer.deserialize_any(SniTargetVisitor)
    }
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

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
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
