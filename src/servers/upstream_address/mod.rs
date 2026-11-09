use log::{debug, error};
use std::fmt::{Display, Formatter};
use std::io::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// How long a successful lookup is reused.
const TTL_OK: Duration = Duration::from_secs(60);
/// After a failed lookup: how long the previous addresses (if any) are kept
/// in use before the next try.
const TTL_ERR: Duration = Duration::from_secs(3);

// ---------------------------------------------------------------------------
// UpstreamAddress — DNS cache with TTL
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct UpstreamAddress {
    address: String,
    cache: Arc<RwLock<Option<Cached>>>,
}

#[derive(Debug)]
struct Cached {
    addresses: Vec<SocketAddr>,
    at: Instant,
    ttl: Duration,
}

impl Cached {
    /// Addresses that may still be used, if any.
    fn fresh(&self) -> Option<&[SocketAddr]> {
        (!self.addresses.is_empty() && self.at.elapsed() < self.ttl).then_some(&self.addresses)
    }
}

impl Display for UpstreamAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.address.fmt(f)
    }
}

impl UpstreamAddress {
    pub fn new(address: String) -> Self {
        UpstreamAddress {
            address,
            ..Default::default()
        }
    }

    /// Cached addresses that are still valid.
    pub(crate) async fn cached(&self) -> Option<Vec<SocketAddr>> {
        let cache = self.cache.read().await;
        cache.as_ref()?.fresh().map(<[_]>::to_vec)
    }

    pub async fn resolve(&self, mode: ResolutionMode) -> Result<Vec<SocketAddr>> {
        if let Some(addresses) = self.cached().await {
            debug!(
                "Using cached addresses for {}: {:?}",
                self.address, addresses
            );
            return Ok(addresses);
        }

        debug!(
            "Resolving addresses for {} with mode {:?}",
            self.address, mode
        );

        let resolved: Vec<SocketAddr> = match tokio::net::lookup_host(&self.address).await {
            Ok(addresses) => addresses.collect(),
            Err(e) => {
                debug!("Failed looking up {}: {}", self.address, e);
                // Keep serving the last good addresses for TTL_ERR, then retry.
                let mut cache = self.cache.write().await;
                match cache.as_mut() {
                    Some(c) => {
                        c.at = Instant::now();
                        c.ttl = TTL_ERR;
                    }
                    None => {
                        *cache = Some(Cached {
                            addresses: Vec::new(),
                            at: Instant::now(),
                            ttl: TTL_ERR,
                        })
                    }
                }
                return Err(e);
            }
        };

        let addresses: Vec<SocketAddr> = match mode {
            ResolutionMode::Ipv4 => resolved.into_iter().filter(|a| a.is_ipv4()).collect(),
            ResolutionMode::Ipv6 => resolved.into_iter().filter(|a| a.is_ipv6()).collect(),
            ResolutionMode::Ipv4AndIpv6 => resolved,
        };

        debug!(
            "Got {} addresses for {}: {:?}",
            mode, self.address, addresses
        );

        *self.cache.write().await = Some(Cached {
            addresses: addresses.clone(),
            at: Instant::now(),
            ttl: TTL_OK,
        });

        Ok(addresses)
    }
}

// ---------------------------------------------------------------------------
// ResolutionMode — controls which address families are used
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub(crate) enum ResolutionMode {
    #[default]
    Ipv4AndIpv6,
    Ipv4,
    Ipv6,
}

impl From<&str> for ResolutionMode {
    fn from(value: &str) -> Self {
        match value {
            "tcp4" => ResolutionMode::Ipv4,
            "tcp6" => ResolutionMode::Ipv6,
            "tcp" => ResolutionMode::Ipv4AndIpv6,
            _ => {
                error!("Unknown protocol '{}', defaulting to IPv4AndIpv6", value);
                ResolutionMode::Ipv4AndIpv6
            }
        }
    }
}

impl Display for ResolutionMode {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolutionMode::Ipv4 => write!(f, "IPv4Only"),
            ResolutionMode::Ipv6 => write!(f, "IPv6Only"),
            ResolutionMode::Ipv4AndIpv6 => write!(f, "IPv4 and IPv6"),
        }
    }
}

#[cfg(test)]
mod tests;
