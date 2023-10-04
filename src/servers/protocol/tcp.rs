use crate::config::Upstream;
use crate::servers::protocol::tls::get_sni;
use crate::servers::{copy, Proxy};
use futures::future::try_join;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::io;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};

pub(crate) async fn proxy(config: Arc<Proxy>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(config.listen).await?;
    let config = config.clone();

    loop {
        let thread_proxy = config.clone();
        match listener.accept().await {
            Err(err) => {
                error!("Failed to accept connection: {}", err);
                return Err(Box::new(err));
            }
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    match accept(stream, thread_proxy).await {
                        Ok(_) => {}
                        Err(err) => {
                            error!("Relay thread returned an error: {}", err);
                        }
                    };
                });
            }
        }
    }
}

async fn accept(inbound: TcpStream, proxy: Arc<Proxy>) -> Result<(), Box<dyn std::error::Error>> {
    info!("New connection from {:?}", inbound.peer_addr()?);

    let upstream_name = match proxy.tls {
        false => proxy.default_action.clone(),
        true => {
            let mut hello_buf = [0u8; 1024];
            inbound.peek(&mut hello_buf).await?;
            let snis = get_sni(&hello_buf);
            if snis.is_empty() {
                proxy.default_action.clone()
            } else {
                match proxy.sni.clone() {
                    Some(sni_map) => {
                        let mut upstream = proxy.default_action.clone();
                        for sni in snis {
                            let m = sni_map.get(&sni);
                            if m.is_some() {
                                upstream = m.unwrap().clone();
                                break;
                            }
                        }
                        upstream
                    }
                    None => proxy.default_action.clone(),
                }
            }
        }
    };

    debug!("Upstream: {}", upstream_name);

    let upstream = match proxy.upstream.get(&upstream_name) {
        Some(upstream) => upstream,
        None => {
            warn!(
                "No upstream named {:?} on server {:?}",
                proxy.default_action, proxy.name
            );
            return process(inbound, proxy.upstream.get(&proxy.default_action).unwrap()).await;
            // ToDo: Remove unwrap and check default option
        }
    };

    process(inbound, upstream).await
}

async fn process(
    mut inbound: TcpStream,
    upstream: &Upstream,
) -> Result<(), Box<dyn std::error::Error>> {
    match upstream {
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
            let outbound = match config.protocol.as_ref() {
                "tcp4" | "tcp6" | "tcp" => {
                    TcpStream::connect(config.resolve_addresses().await?.as_slice()).await?
                }
                _ => {
                    error!("Reached unknown protocol: {:?}", config.protocol);
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
        }
    };
    Ok(())
}
