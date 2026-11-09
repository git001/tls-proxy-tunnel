use log::{error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
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
    /// How long a shutdown waits for active connections (ZERO = no limit).
    pub shutdown_timeout: Duration,
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
    /// TLS only: deadline for the client's ClientHello (ZERO = none).
    pub client_hello_timeout: Duration,
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
        // Set when a listener fails (bind or accept). The whole process then
        // shuts down and run() returns Err, so the exit code shows the failure
        // instead of tpt running on without that listener.
        let failed = Arc::new(AtomicBool::new(false));

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
            let failed = failed.clone();
            tracker.spawn(async move {
                let result = match config.protocol.as_ref() {
                    "tcp" | "tcp4" | "tcp6" => {
                        tcp::proxy(config.clone(), token.clone(), tracker_clone).await
                    }
                    other => Err(format!("Invalid protocol: {other}").into()),
                };
                if let Err(e) = result {
                    error!("Server {} on {} failed: {}", config.name, config.listen, e);
                    failed.store(true, Ordering::SeqCst);
                    token.cancel();
                }
            });
        }

        // Block until a signal or a failed listener cancels the token.
        token.cancelled().await;
        info!("Shutdown signal received, waiting for active connections to close...");

        // Stop the tracker from accepting new spawns, then wait for all
        // in-flight proxy loops and connection tasks to finish — at most
        // shutdown_timeout, so tpt exits before Kubernetes sends SIGKILL.
        tracker.close();
        let drained = if self.shutdown_timeout.is_zero() {
            tracker.wait().await;
            true
        } else {
            tokio::time::timeout(self.shutdown_timeout, tracker.wait())
                .await
                .is_ok()
        };
        if !drained {
            let open: usize = self
                .proxies
                .iter()
                .map(|p| p.maxclients_limit - p.maxclients.available_permits())
                .sum();
            warn!(
                "shutdown_timeout {:?} reached with {} connection(s) still open; closing them",
                self.shutdown_timeout, open
            );
        }

        if failed.load(Ordering::SeqCst) {
            return Err("a listener failed, see the log above".into());
        }
        info!("Shutdown complete.");
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
