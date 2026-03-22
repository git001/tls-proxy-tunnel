use log::{error, info};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::Semaphore;
use tokio::task;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

mod builder;
mod protocol;
pub(crate) mod upstream_address;

use crate::config::SniTarget;
use crate::config::ViaUpstream;
use crate::upstreams::Upstream;
use protocol::tcp;

pub(super) type UpstreamMap = Arc<HashMap<String, Upstream>>;

#[derive(Debug)]
pub(crate) struct Server {
    pub proxies: Vec<Arc<Proxy>>,
}

#[derive(Debug, Clone)]
pub(crate) struct Proxy {
    pub name: String,
    pub listen: SocketAddr,
    pub protocol: String,
    pub tls: bool,
    pub sni: Option<HashMap<String, SniTarget>>,
    pub default_action: String,
    pub upstream: UpstreamMap,
    pub via: ViaUpstream,
    pub maxclients: Arc<Semaphore>,
    /// Maximum number of concurrent connections (config value).
    pub maxclients_limit: usize,
}

impl Proxy {
    pub fn is_health_server(&self) -> bool {
        matches!(
            self.upstream.get(&self.default_action),
            Some(Upstream::Health(_))
        )
    }
}

impl Server {
    #[tokio::main]
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let proxies = self.proxies.clone();
        let token = CancellationToken::new();
        let tracker = TaskTracker::new();

        // Signal handlers cancel the token instead of calling process::exit.
        // This lets active connections drain before the process exits.
        for sig in [
            SignalKind::interrupt(),
            SignalKind::terminate(),
            SignalKind::hangup(),
            SignalKind::quit(),
        ] {
            let token = token.clone();
            task::spawn(async move {
                let mut listener = signal(sig).expect("Failed to initialize a signal handler");
                listener.recv().await;
                info!("SIG received {:?}: initiating graceful shutdown.", sig);
                token.cancel();
            });
        }

        for config in proxies {
            info!(
                "Starting {} server {} on {}",
                config.protocol, config.name, config.listen
            );
            let token = token.clone();
            let tracker_clone = tracker.clone();
            tracker.spawn(async move {
                match config.protocol.as_ref() {
                    "tcp" | "tcp4" | "tcp6" => {
                        if let Err(e) = tcp::proxy(config.clone(), token, tracker_clone).await {
                            error!("Failed to start {}: {}", config.name, e);
                        }
                    }
                    _ => {
                        error!("Invalid protocol: {}", config.protocol)
                    }
                }
            });
        }

        // Block until a signal fires and cancels the token.
        token.cancelled().await;
        info!("Shutdown signal received, waiting for active connections to close...");

        // Stop the tracker from accepting new spawns, then wait for all
        // in-flight proxy loops and connection tasks to finish.
        tracker.close();
        tracker.wait().await;

        info!("Shutdown complete.");
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
