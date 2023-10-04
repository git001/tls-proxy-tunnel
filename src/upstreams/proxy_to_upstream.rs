use crate::servers::upstream_address::UpstreamAddress;

use crate::upstreams::copy;
use futures::future::try_join;
use log::{debug, error};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio::io;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct Addr(Mutex<UpstreamAddress>);

impl Clone for Addr {
    fn clone(&self) -> Self {
        tokio::task::block_in_place(|| Self(Mutex::new(self.0.blocking_lock().clone())))
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProxyToUpstream {
    pub addr: String,
    pub protocol: String,
    #[serde(skip_deserializing)]
    addresses: Addr,
}

impl ProxyToUpstream {
    pub async fn resolve_addresses(&self) -> std::io::Result<Vec<SocketAddr>> {
        let mut addr = self.addresses.0.lock().await;
        addr.resolve((*self.protocol).into()).await
    }

    pub fn new(address: String, protocol: String) -> Self {
        Self {
            addr: address.clone(),
            protocol,
            addresses: Addr(Mutex::new(UpstreamAddress::new(address))),
        }
    }

    pub(crate) async fn proxy(&self, inbound: TcpStream) -> Result<(), Box<dyn std::error::Error>> {
        let outbound = match self.protocol.as_ref() {
            "tcp4" | "tcp6" | "tcp" => {
                TcpStream::connect(self.resolve_addresses().await?.as_slice()).await?
            }
            _ => {
                error!("Reached unknown protocol: {:?}", self.protocol);
                return Err("Reached unknown protocol".into());
            }
        };

        debug!("Connected to {:?}", outbound.peer_addr().unwrap());

        let (mut ri, mut wi) = io::split(inbound);
        let (mut ro, mut wo) = io::split(outbound);

        let inbound_to_outbound = copy(&mut ri, &mut wo);
        let outbound_to_inbound = copy(&mut ro, &mut wi);

        let (bytes_tx, bytes_rx) = try_join(inbound_to_outbound, outbound_to_inbound).await?;

        debug!("Bytes read: {:?} write: {:?}", bytes_tx, bytes_rx);

        Ok(())
    }
}
