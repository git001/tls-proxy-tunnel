use crate::servers::upstream_address::UpstreamAddress;
use log::{debug, error};
use std::error::Error;
use std::time::Duration;
use tokio::net::TcpStream;

use super::ProxyError;

// ---------------------------------------------------------------------------
// Connect to the upstream TCP server with a timeout.
// ---------------------------------------------------------------------------
pub(super) async fn connect_upstream(
    addr: &str,
    addresses: &UpstreamAddress,
    protocol: &str,
    timeout: Duration,
) -> Result<TcpStream, Box<dyn Error>> {
    match protocol {
        "tcp4" | "tcp6" | "tcp" => {}
        _ => {
            error!("Reached unknown protocol: {:?}", protocol);
            return Err("Reached unknown protocol".into());
        }
    }

    match tokio::time::timeout(
        timeout,
        TcpStream::connect(addresses.resolve(protocol.into()).await?.as_slice()),
    )
    .await
    {
        Ok(Ok(stream)) => {
            debug!("Connected to {:?}", stream.peer_addr());
            Ok(stream)
        }
        Ok(Err(e)) => {
            error!("Failed to connect to upstream {}: {:?}", addr, e);
            Err(ProxyError(format!("failed to connect to upstream: {}", e)).into())
        }
        Err(e) => {
            error!("Connection to upstream {} timed out: {:?}", addr, e);
            Err(ProxyError(format!("connection to upstream timed out: {:?}", e)).into())
        }
    }
}

#[cfg(test)]
#[path = "connect_tests.rs"]
mod tests;
