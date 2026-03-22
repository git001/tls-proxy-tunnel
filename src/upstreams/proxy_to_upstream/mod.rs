use crate::config::ViaUpstream;
use crate::servers::upstream_address::UpstreamAddress;
use log::{debug, info};
use std::error::Error;
use std::fmt;
use tokio::net::TcpStream;

mod connect;
mod http;
mod relay;

// ---------------------------------------------------------------------------
// Shared error type used across submodules.
// ---------------------------------------------------------------------------
#[derive(Debug)]
pub(super) struct ProxyError(pub String);

impl fmt::Display for ProxyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "proxy error: {}", self.0)
    }
}

impl Error for ProxyError {}

// ---------------------------------------------------------------------------
// Public struct
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, Default)]
pub struct ProxyToUpstream {
    pub addr: String,
    pub protocol: String,
    addresses: UpstreamAddress,
}

impl ProxyToUpstream {
    pub fn new(address: String, protocol: String) -> Self {
        Self {
            addr: address.clone(),
            protocol,
            addresses: UpstreamAddress::new(address),
        }
    }

    pub(crate) async fn proxy(
        &self,
        inbound: TcpStream,
        via: &ViaUpstream,
        connect_target: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        let outbound = connect::connect_upstream(
            &self.addr,
            &self.addresses,
            &self.protocol,
            via.connect_timeout,
        )
        .await?;

        outbound.set_nodelay(true)?;
        inbound.set_nodelay(true)?;

        let label = match &connect_target {
            Some(t) => format!("{} → {}", self.addr, t),
            None => format!("{} (direct)", self.addr),
        };

        match connect_target {
            None => {
                debug!("No CONNECT target — direct TCP forward to {}", self.addr);
                let (tx, rx) = relay::relay(inbound, outbound, label, via.stats_interval).await?;
                info!(
                    "Direct forward complete: tx={} rx={} upstream={}",
                    tx, rx, self.addr
                );
            }
            Some(target) => {
                debug!(
                    "HTTP CONNECT target={:?} via headers={:?}",
                    target, via.headers
                );
                http::http_connect(&outbound, &target, &via.headers).await?;
                let (tx, rx) = relay::relay(inbound, outbound, label, via.stats_interval).await?;
                info!(
                    "CONNECT tunnel complete: tx={} rx={} target={:?}",
                    tx, rx, target
                );
            }
        }

        Ok(())
    }
}
