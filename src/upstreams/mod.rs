mod proxy_to_upstream;

use log::debug;
use serde::Deserialize;
use std::error::Error;
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub use crate::upstreams::proxy_to_upstream::ProxyToUpstream;

#[derive(Debug, Clone, Deserialize)]
pub enum Upstream {
    Ban,
    Echo,
    Proxy(ProxyToUpstream),
}

impl Upstream {
    pub(crate) async fn process(&self, mut inbound: TcpStream) -> Result<(), Box<dyn Error>> {
        match self {
            Upstream::Ban => {
                inbound.shutdown().await?;
            }
            Upstream::Echo => {
                let (mut ri, mut wi) = io::split(inbound);
                let inbound_to_inbound = copy(&mut ri, &mut wi);
                let bytes_tx = inbound_to_inbound.await;
                debug!("Bytes read: {:?}", bytes_tx);
            }
            Upstream::Proxy(config) => {
                config.proxy(inbound).await?;
            }
        };
        Ok(())
    }
}

async fn copy<'a, R, W>(reader: &'a mut R, writer: &'a mut W) -> io::Result<u64>
where
    R: AsyncRead + Unpin + ?Sized,
    W: AsyncWrite + Unpin + ?Sized,
{
    match io::copy(reader, writer).await {
        Ok(u64) => {
            let _ = writer.shutdown().await;
            Ok(u64)
        }
        Err(_) => Ok(0),
    }
}
