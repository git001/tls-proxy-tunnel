use crate::servers::Proxy;
use crate::servers::protocol::tls::get_sni;
use log::{debug, error, info, warn};
use std::error::Error;
use std::sync::Arc;
use tokio::{
    io::{self},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub(crate) async fn proxy(
    config: Arc<Proxy>,
    token: CancellationToken,
    tracker: TaskTracker,
) -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind(config.listen).await?;

    debug!(
        "Name :{:?}: Semaphore :{:?}:",
        config.name, config.maxclients
    );

    // Health servers bypass maxclients entirely — health checks must always
    // succeed regardless of connection load on the same instance.
    let is_health_server = config.is_health_server();

    loop {
        // Wait for either an incoming connection or a shutdown signal.
        let (stream, peer) = tokio::select! {
            result = listener.accept() => match result {
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    return Err(Box::new(e));
                }
                Ok(pair) => pair,
            },
            _ = token.cancelled() => {
                info!("Listener '{}' shutting down, no longer accepting connections.", config.name);
                return Ok(());
            }
        };

        let thread_proxy = config.clone();

        if is_health_server {
            // No permit needed — health checks are never counted against maxclients.
            tracker.spawn(async move {
                if let Err(e) = accept(stream, thread_proxy).await {
                    error!("Health handler error: {}", e);
                }
            });
            continue;
        }

        // Try to acquire a permit without blocking. If the server is at
        // capacity the incoming connection is dropped immediately (TCP RST),
        // which is a clear signal to the caller rather than silently queuing
        // in the OS backlog.
        let permit = match config.maxclients.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                warn!(
                    "maxclients reached on '{}', rejecting connection from {}",
                    config.name, peer,
                );
                // stream is dropped here → connection is closed
                continue;
            }
        };

        tracker.spawn(async move {
            if let Err(e) = accept(stream, thread_proxy).await {
                error!("Relay thread returned an error: {}", e);
            }
            drop(permit);
        });
    }
}

async fn accept(inbound: TcpStream, proxy: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
    let is_health = proxy.is_health_server();

    if is_health {
        debug!("Health check request");
    } else {
        let active = proxy
            .maxclients_limit
            .saturating_sub(proxy.maxclients.available_permits());
        info!(
            "New connection from {:?}, active: {}/{}",
            inbound.peer_addr()?,
            active,
            proxy.maxclients_limit
        );
    }

    // For TLS connections: peek at the ClientHello to extract SNI.
    // peek() does not consume bytes — the ClientHello is replayed automatically
    // once the bidirectional copy starts, so the TLS handshake runs end-to-end.
    let snis: Vec<String> = if proxy.tls {
        let mut hello_buf = [0u8; 4096];
        inbound.peek(&mut hello_buf).await?;
        get_sni(&hello_buf)
    } else {
        Vec::new()
    };

    // Route to the upstream name based on SNI map (or fall back to default).
    // Also extract any per-SNI via override from extended SniTarget entries.
    let (upstream_name, sni_via_override) = if !snis.is_empty() {
        match &proxy.sni {
            Some(sni_map) => {
                let mut matched = proxy.default_action.clone();
                let mut via_override = None;
                for sni in &snis {
                    if let Some(target) = sni_map.get(sni) {
                        matched = target.upstream_name().to_string();
                        via_override = target.via_override();
                        break;
                    }
                }
                (matched, via_override)
            }
            None => (proxy.default_action.clone(), None),
        }
    } else {
        (proxy.default_action.clone(), None)
    };

    // Per-SNI via takes precedence over the server-level via.
    let effective_via = sni_via_override.unwrap_or(&proxy.via);

    // Determine the CONNECT target for the upstream HTTP proxy:
    //   use_sni_as_target=true  → "{first_sni}:{target_port}" (dynamic, per-connection)
    //   use_sni_as_target=false → via.target if non-empty (static config)
    //   otherwise               → None (direct TCP forward, no CONNECT)
    let connect_target: Option<String> = if effective_via.use_sni_as_target {
        snis.first()
            .map(|sni| format!("{}:{}", sni, effective_via.target_port))
    } else if !effective_via.target.is_empty() {
        Some(effective_via.target.clone())
    } else {
        None
    };

    debug!(
        "Upstream: {} connect_target: {:?}",
        upstream_name, connect_target
    );

    let upstream = match proxy.upstream.get(&upstream_name) {
        Some(upstream) => upstream,
        None => {
            warn!(
                "No upstream named {:?} on server {:?}, falling back to default",
                upstream_name, proxy.name
            );
            match proxy.upstream.get(&proxy.default_action) {
                Some(u) => u,
                None => {
                    return Err(format!(
                        "default upstream '{}' not found on server '{}'",
                        proxy.default_action, proxy.name
                    )
                    .into());
                }
            }
        }
    };

    let result = upstream
        .process(inbound, effective_via, connect_target)
        .await;

    if !is_health {
        let active = proxy
            .maxclients_limit
            .saturating_sub(proxy.maxclients.available_permits());
        match &result {
            Ok(_) => info!(
                "Connection closed for {:?}, active: {}/{}",
                upstream_name, active, proxy.maxclients_limit
            ),
            Err(e) => error!(
                "Connection error for {:?}, active: {}/{}: {:?}",
                upstream_name, active, proxy.maxclients_limit, e
            ),
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
