use log::{debug, error};
use std::fmt::{Display, Formatter};
use std::io::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use time::{Duration, OffsetDateTime};
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// UpstreamAddress — DNS cache with TTL
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub(crate) struct UpstreamAddress {
    address: String,
    resolved_addresses: Arc<RwLock<Vec<SocketAddr>>>,
    resolved_time: Arc<RwLock<Option<Instant>>>,
    ttl: Arc<RwLock<Option<Duration>>>,
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

    pub(crate) async fn is_valid(&self) -> bool {
        let r = *self.resolved_time.read().await;
        if let Some(resolved) = r
            && let Some(ttl) = *self.ttl.read().await
        {
            return resolved.elapsed() < ttl;
        }
        false
    }

    async fn is_resolved(&self) -> bool {
        !self.resolved_addresses.read().await.is_empty()
    }

    async fn time_remaining(&self) -> Duration {
        if !self.is_valid().await {
            return Duration::seconds(0);
        }
        let rt = *self.resolved_time.read().await;
        let ttl = *self.ttl.read().await;
        match (ttl, rt) {
            (Some(t), Some(r)) => t - r.elapsed(),
            _ => Duration::seconds(0),
        }
    }

    pub async fn resolve(&self, mode: ResolutionMode) -> Result<Vec<SocketAddr>> {
        if self.is_resolved().await && self.is_valid().await {
            debug!(
                "Already got address {:?}, still valid for {:.3}s",
                &self.resolved_addresses,
                self.time_remaining().await.as_seconds_f64()
            );
            return Ok(self.resolved_addresses.read().await.clone());
        }

        debug!(
            "Resolving addresses for {} with mode {:?}",
            &self.address, &mode
        );

        let lookup_result = tokio::net::lookup_host(&self.address).await;

        let resolved_addresses: Vec<SocketAddr> = match lookup_result {
            Ok(resolved_addresses) => resolved_addresses.into_iter().collect(),
            Err(e) => {
                debug!("Failed looking up {}: {}", &self.address, &e);
                *self.resolved_time.write().await = Some(Instant::now());
                *self.ttl.write().await = Some(Duration::seconds(3));
                return Err(e);
            }
        };

        debug!("Resolved addresses: {:?}", &resolved_addresses);

        let addresses: Vec<SocketAddr> = match mode {
            ResolutionMode::Ipv4 => resolved_addresses
                .into_iter()
                .filter(|a| a.is_ipv4())
                .collect(),

            ResolutionMode::Ipv6 => resolved_addresses
                .into_iter()
                .filter(|a| a.is_ipv6())
                .collect(),

            _ => resolved_addresses,
        };

        debug!(
            "Got {} addresses for {}: {:?}",
            &mode, &self.address, &addresses
        );
        debug!(
            "Resolved at {}",
            OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .expect("Format")
        );

        self.resolved_addresses.write().await.clone_from(&addresses);
        *self.resolved_time.write().await = Some(Instant::now());
        *self.ttl.write().await = Some(Duration::minutes(1));

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
