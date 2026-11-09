use crate::servers::Proxy;
use crate::servers::protocol::tls::get_sni;
use log::{debug, error, info, warn};
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{self, AsyncRead, AsyncReadExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Largest TLS record: 5-byte header + 2^14 bytes payload.
const MAX_TLS_RECORD: usize = 5 + 16 * 1024;

/// Pause after a failed accept() before the next try.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

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
                Ok(pair) => pair,
                Err(e) => {
                    // accept() fails per connection (ECONNABORTED) or when
                    // resources run out (EMFILE, ENFILE, ENOBUFS); the
                    // listener stays usable. Back off so a full fd table
                    // does not spin the loop, then try again.
                    error!("Failed to accept connection on '{}': {}", config.name, e);
                    tokio::select! {
                        _ = tokio::time::sleep(ACCEPT_BACKOFF) => continue,
                        _ = token.cancelled() => return Ok(()),
                    }
                }
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

/// Read the client's first TLS record (the ClientHello) completely.
///
/// A ClientHello often spans several TCP segments (post-quantum key shares
/// push it past 1500 bytes), so one read or peek can see only part of it
/// and miss the SNI. The bytes are consumed; the upstream sends them on
/// first. Stops early at EOF, for data that is not a TLS handshake record
/// (the caller routes it to the default), and at MAX_TLS_RECORD.
pub(super) async fn read_client_hello<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(2048);
    loop {
        let want = match buf[..] {
            [first, ..] if first != 0x16 => return Ok(buf),
            [_, _, _, hi, lo, ..] => {
                (5 + u16::from_be_bytes([hi, lo]) as usize).min(MAX_TLS_RECORD)
            }
            _ => 5,
        };
        if buf.len() >= want {
            return Ok(buf);
        }
        let missing = (want - buf.len()) as u64;
        if (&mut *r).take(missing).read_buf(&mut buf).await? == 0 {
            return Ok(buf); // EOF before the record was complete
        }
    }
}

async fn accept(mut inbound: TcpStream, proxy: Arc<Proxy>) -> Result<(), Box<dyn Error>> {
    let is_health = proxy.is_health_server();
    let peer = inbound.peer_addr()?;

    if is_health {
        debug!("Health check request");
    } else {
        let active = proxy
            .maxclients_limit
            .saturating_sub(proxy.maxclients.available_permits());
        info!(
            "New connection from {:?}, active: {}/{}",
            peer, active, proxy.maxclients_limit
        );
    }

    // TLS: read the ClientHello for the SNI. Health servers speak plain HTTP.
    let client_hello = if proxy.tls && !is_health {
        let read = read_client_hello(&mut inbound);
        let result = if proxy.client_hello_timeout.is_zero() {
            Ok(read.await)
        } else {
            tokio::time::timeout(proxy.client_hello_timeout, read).await
        };
        match result {
            Ok(hello) => hello?,
            Err(_) => {
                warn!(
                    "No TLS ClientHello from {} within {:?} on '{}', closing",
                    peer, proxy.client_hello_timeout, proxy.name
                );
                return Ok(());
            }
        }
    } else {
        Vec::new()
    };
    let snis = get_sni(&client_hello);

    // Route by the first SNI found in the map, else the default.
    // An extended SNI entry may carry its own via (it wins over the server's).
    let route = snis.iter().find_map(|sni| proxy.sni.as_ref()?.get(sni));
    let (upstream_name, sni_via_override) = match route {
        Some(target) => (target.upstream_name(), target.via_override()),
        None => (proxy.default_action.as_str(), None),
    };
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

    // verify_config() guarantees every name in the config exists.
    let upstream = proxy.upstream.get(upstream_name).ok_or_else(|| {
        format!(
            "upstream '{}' not found on server '{}'",
            upstream_name, proxy.name
        )
    })?;

    let result = upstream
        .process(inbound, &client_hello, effective_via, connect_target)
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
