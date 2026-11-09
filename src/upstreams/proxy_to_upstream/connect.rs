use crate::servers::upstream_address::UpstreamAddress;
use log::{debug, error};
use std::error::Error;
use tokio::net::TcpStream;

use super::ProxyError;

// ---------------------------------------------------------------------------
// Resolve and connect to the upstream. The caller bounds this with
// via.connect_timeout (together with the CONNECT exchange).
// ---------------------------------------------------------------------------
pub(super) async fn connect_upstream(
    addresses: &UpstreamAddress,
    protocol: &str,
) -> Result<TcpStream, Box<dyn Error>> {
    match protocol {
        "tcp4" | "tcp6" | "tcp" => {}
        _ => {
            error!("Reached unknown protocol: {:?}", protocol);
            return Err("Reached unknown protocol".into());
        }
    }

    let resolved = addresses.resolve(protocol.into()).await?;
    match TcpStream::connect(resolved.as_slice()).await {
        Ok(stream) => {
            debug!("Connected to {:?}", stream.peer_addr());
            Ok(stream)
        }
        Err(e) => {
            error!("Failed to connect to upstream {}: {:?}", addresses, e);
            Err(ProxyError(format!("failed to connect to upstream: {}", e)).into())
        }
    }
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;
