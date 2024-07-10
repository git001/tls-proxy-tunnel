use crate::servers::protocol::tls::get_sni;
use crate::servers::Proxy;
use crate::GLOBAL_THREAD_COUNT;
use log::{debug, error, info, warn};
use std::error::Error;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::{
    io::{self},
    net::{TcpListener, TcpStream},
};

pub(crate) async fn proxy(config: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(config.listen).await?;
    let config = config.clone();

    debug!(
        "Name :{:?}: Semaphore :{:?}:",
        config.name, config.maxclients
    );

    // Put the drop inside the tokio::spawn after the call to accept
    // Big thanks to alice from https://users.rust-lang.org/

    loop {
        let thread_proxy = config.clone();
        let permit = config.maxclients.clone().acquire_owned().await.unwrap();

        match listener.accept().await {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                continue;
            }
            Err(err) => {
                error!("Failed to accept connection: {}", err);
                return Err(Box::new(err));
            }
            Ok((stream, _)) => {
                tokio::spawn(async move {
                    match accept(stream, thread_proxy).await {
                        Ok(_) => {
                            debug!("Accepted permit {:?}", permit);
                        }
                        Err(err) => {
                            error!("Relay thread returned an error: {}", err);
                        }
                    };
                    drop(permit);
                });
            }
        }
    }
}

async fn accept(inbound: TcpStream, proxy: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
    if proxy.default_action.contains("health") {
        debug!("Health check request")
    } else {
        let old = GLOBAL_THREAD_COUNT.fetch_add(1, Ordering::SeqCst);
        info!(
            "New connection from {:?} , old :{:?}: Current Connections :{:?}",
            inbound.peer_addr()?,
            old,
            GLOBAL_THREAD_COUNT
        );
    }

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
                            let m: Option<&String> = sni_map.get(&sni);
                            if let Some(value) = m {
                                upstream.clone_from(value);
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

    match upstream.process(inbound, proxy.clone()).await {
        Ok(_) => {
            if proxy.default_action.contains("health") {
                debug!("Health check request");
                Ok(())
            } else {
                let old = GLOBAL_THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
                info!(
                    "OKAY: Connection closed for {:?}, old :{:?}: Current Connections :{:?}",
                    upstream_name, old, GLOBAL_THREAD_COUNT
                );
                Ok(())
            }
        }
        Err(e) => {
            let old = GLOBAL_THREAD_COUNT.fetch_sub(1, Ordering::SeqCst);
            info!(
                "ERROR: Connection closed for {:?}, num :{:?}: Current Connections :{:?}",
                upstream_name, old, GLOBAL_THREAD_COUNT
            );
            error!("my error {:?}", e);
            Ok(())
        }
    }
}
