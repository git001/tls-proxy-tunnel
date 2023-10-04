use crate::servers::protocol::tls::get_sni;
use crate::servers::Proxy;
use log::{debug, error, info, warn};
use std::error::Error;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

pub(crate) async fn proxy(config: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
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

async fn accept(inbound: TcpStream, proxy: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
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
            proxy.upstream.get(&proxy.default_action).unwrap()
        }
    };

    upstream.process(inbound).await
}
