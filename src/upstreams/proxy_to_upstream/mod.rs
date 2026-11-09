use crate::config::ViaUpstream;
use crate::servers::upstream_address::UpstreamAddress;
use log::{debug, info};
use socket2::{SockRef, TcpKeepalive};
use std::error::Error;
use std::fmt;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
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
        mut inbound: TcpStream,
        client_hello: &[u8],
        via: &ViaUpstream,
        connect_target: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
        // DNS lookup, TCP connect and the CONNECT exchange share one
        // deadline: a proxy that accepts but never answers must not hold
        // the connection (and its maxclients slot) forever.
        let setup = async {
            let mut outbound = connect::connect_upstream(&self.addresses, &self.protocol).await?;
            let early = match &connect_target {
                Some(target) => {
                    debug!(
                        "HTTP CONNECT target={:?} via headers={:?}",
                        target, via.headers
                    );
                    http::http_connect(&mut outbound, target, &via.headers).await?
                }
                None => {
                    debug!("No CONNECT target — direct TCP forward to {}", self.addr);
                    Vec::new()
                }
            };
            Ok::<_, Box<dyn Error>>((outbound, early))
        };
        // Own scope: the Result holds a non-Send Box<dyn Error> and must be
        // gone before the next await.
        let (mut outbound, early) = {
            let ready = if via.connect_timeout.is_zero() {
                Ok(setup.await)
            } else {
                tokio::time::timeout(via.connect_timeout, setup).await
            };
            match ready {
                Ok(result) => result?,
                Err(_) => {
                    return Err(ProxyError(format!(
                        "upstream {} not ready within {:?} (DNS, connect, CONNECT)",
                        self.addr, via.connect_timeout
                    ))
                    .into());
                }
            }
        };

        for stream in [&inbound, &outbound] {
            stream.set_nodelay(true)?;
            set_keepalive(stream);
        }

        // Bytes read before the relay starts: the client's ClientHello goes
        // up, anything the proxy sent after its CONNECT response goes down
        // (a server that speaks first, e.g. an SSH or SMTP banner).
        outbound.write_all(client_hello).await?;
        inbound.write_all(&early).await?;

        let label = match &connect_target {
            Some(t) => format!("{} → {}", self.addr, t),
            None => format!("{} (direct)", self.addr),
        };
        let (tx, rx) = relay::relay(
            inbound,
            outbound,
            label.clone(),
            via.stats_interval,
            via.idle_timeout,
        )
        .await?;
        info!(
            "Tunnel complete: tx={} rx={} {}",
            tx + client_hello.len() as u64,
            rx + early.len() as u64,
            label
        );
        Ok(())
    }
}

/// TCP keepalive, so a peer that vanished without FIN or RST (NAT timeout,
/// cable pulled) is detected after ~90s instead of the kernel default of
/// 2h+ and its connection and maxclients slot are freed.
pub(super) fn set_keepalive(stream: &TcpStream) {
    let keepalive = TcpKeepalive::new().with_time(Duration::from_secs(60));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let keepalive = keepalive
        .with_interval(Duration::from_secs(10))
        .with_retries(3);
    if let Err(e) = SockRef::from(stream).set_tcp_keepalive(&keepalive) {
        debug!("Could not set TCP keepalive: {}", e);
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
