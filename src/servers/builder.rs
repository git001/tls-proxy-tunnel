use log::{debug, error};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::config::ParsedConfig;
use crate::upstreams::{Metrics, MetricsEntry, Upstream};

use super::{Proxy, Server, UpstreamMap};

impl From<ParsedConfig> for Server {
    fn from(config: ParsedConfig) -> Self {
        // Pass 1: build per-proxy semaphores — needed before metrics can be created.
        let mut raw_proxies: Vec<Proxy> = Vec::new();

        for (name, proxy_cfg) in config.servers.iter() {
            let protocol = proxy_cfg
                .protocol
                .clone()
                .unwrap_or_else(|| "tcp".to_string());
            let tls = proxy_cfg.tls.unwrap_or(false);
            let sni = proxy_cfg.sni.clone();
            let default = proxy_cfg
                .default
                .clone()
                .unwrap_or_else(|| "ban".to_string());
            let maxclients_limit = proxy_cfg.maxclients;

            for listen in proxy_cfg.listen.clone() {
                let listen_addr: SocketAddr = match listen.parse() {
                    Ok(addr) => addr,
                    Err(_) => {
                        error!("Invalid listen address: {}", listen);
                        continue;
                    }
                };

                debug!("proxy.maxclients {:?}", maxclients_limit);

                raw_proxies.push(Proxy {
                    name: name.clone(),
                    listen: listen_addr,
                    protocol: protocol.clone(),
                    tls,
                    sni: sni.clone(),
                    default_action: default.clone(),
                    // Placeholder — replaced with the real shared Arc in Pass 3.
                    upstream: Arc::new(HashMap::new()),
                    via: proxy_cfg.via.clone(),
                    maxclients: Arc::new(Semaphore::new(maxclients_limit)),
                    maxclients_limit,
                });
            }
        }

        // Pass 2: build a shared Metrics snapshot from all proxies.
        let metrics: Metrics = Arc::new(
            raw_proxies
                .iter()
                .map(|p| MetricsEntry {
                    name: p.name.clone(),
                    listen: p.listen.to_string(),
                    maxclients_limit: p.maxclients_limit,
                    semaphore: p.maxclients.clone(),
                })
                .collect(),
        );

        // Pass 3: build one shared upstream map with real metrics injected,
        // then hand the same Arc to every proxy — no HashMap clone per proxy.
        let upstream: UpstreamMap = Arc::new(
            config
                .upstream
                .into_iter()
                .map(|(k, v)| {
                    let v = if matches!(v, Upstream::Health(_)) {
                        Upstream::Health(metrics.clone())
                    } else {
                        v
                    };
                    (k, v)
                })
                .collect(),
        );

        for proxy in raw_proxies.iter_mut() {
            proxy.upstream = upstream.clone();
        }

        Server {
            proxies: raw_proxies.into_iter().map(Arc::new).collect(),
        }
    }
}
