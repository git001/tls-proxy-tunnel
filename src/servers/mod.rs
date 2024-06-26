use log::{debug, error, info};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Semaphore;

use tokio::signal::unix::{signal, SignalKind};
use tokio::task;
use tokio::task::JoinHandle;
use tokio_util::task::TaskTracker;

mod protocol;
pub(crate) mod upstream_address;

use crate::config::ParsedConfigV1;
use crate::config::ViaUpstream;
use crate::upstreams::Upstream;
use protocol::tcp;

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
    pub sni: Option<HashMap<String, String>>,
    pub default_action: String,
    pub upstream: HashMap<String, Upstream>,
    pub via: ViaUpstream,
    pub maxclients: Arc<Semaphore<>>,
    //pub maxclients: usize,
}

impl Server {
    pub fn new_from_v1_config(config: ParsedConfigV1) -> Self {
        let mut new_server = Server {
            proxies: Vec::new(),
        };

        for (name, proxy) in config.servers.iter() {
            let protocol = proxy.protocol.clone().unwrap_or_else(|| "tcp".to_string());
            let tls = proxy.tls.unwrap_or(false);
            let sni = proxy.sni.clone();
            let default = proxy.default.clone().unwrap_or_else(|| "ban".to_string());
            let upstream = config.upstream.clone();
            let mut upstream_set: HashSet<String> = HashSet::new();
            for key in upstream.keys() {
                if key.eq("ban") || key.eq("echo") {
                    continue;
                }
                upstream_set.insert(key.clone());
            }
            for listen in proxy.listen.clone() {
                let listen_addr: SocketAddr = match listen.parse() {
                    Ok(addr) => addr,
                    Err(_) => {
                        error!("Invalid listen address: {}", listen);
                        continue;
                    }
                };

                debug!("proxy.maxclients {:?}", proxy.maxclients);

                let proxy = Proxy {
                    name: name.clone(),
                    listen: listen_addr,
                    protocol: protocol.clone(),
                    tls,
                    sni: sni.clone(),
                    default_action: default.clone(),
                    upstream: upstream.clone(),
                    via: proxy.via.clone(),
                    maxclients: Arc::new(Semaphore::new(proxy.maxclients)),
                    //maxclients: proxy.maxclients,
                };
                new_server.proxies.push(Arc::new(proxy));
            }
        }

        new_server
    }

    #[tokio::main]
    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let proxies = self.proxies.clone();
        let mut handles: Vec<JoinHandle<()>> = Vec::new();
        let tracker = TaskTracker::new();

        for config in proxies {
            info!(
                "Starting {} server {} on {}",
                config.protocol, config.name, config.listen
            );
            let handle = tokio::spawn(async move {
                match config.protocol.as_ref() {
                    "tcp" | "tcp4" | "tcp6" => {
                        let res = tcp::proxy(config.clone()).await;
                        if res.is_err() {
                            error!("Failed to start {}: {}", config.name, res.err().unwrap());
                        }
                    }
                    _ => {
                        error!("Invalid protocol: {}", config.protocol)
                    }
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.await?;
        }

        // Once we spawned everything, we close the tracker.
        tracker.close();

        // Wait for everything to finish.
        tracker.wait().await;

        for sig in [
            SignalKind::interrupt(),
            SignalKind::terminate(),
            SignalKind::hangup(),
            SignalKind::quit(),
        ] {
            task::spawn(async move {
                let mut listener = signal(sig).expect("Failed to initialize a signal handler");
                info!("SIG received :{:?}:, terminating.", sig);
                listener.recv().await;
                // At this point we've received SIGINT/SIGKILL and we can shut down
                std::process::exit(0);
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::thread::{self, sleep};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[tokio::main]
    async fn tcp_mock_server() {
        let server_addr: SocketAddr = "127.0.0.1:54599".parse().unwrap();
        let listener = TcpListener::bind(server_addr).await.unwrap();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 2];
            let mut n = stream.read(&mut buf).await.unwrap();
            while n > 0 {
                let _ = stream.write(b"hello").await.unwrap();
                if buf.eq(b"by") {
                    stream.shutdown().await.unwrap();
                    break;
                }
                n = stream.read(&mut buf).await.unwrap();
            }
            stream.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_proxy() {
        use crate::config::ConfigV1;
        let config = ConfigV1::new("tests/config.yaml").unwrap();
        let mut server = Server::new_from_v1_config(config.base);
        thread::spawn(move || {
            tcp_mock_server();
        });
        sleep(Duration::from_secs(1)); // wait for server to start
        thread::spawn(move || {
            let _ = server.run();
        });
        sleep(Duration::from_secs(1)); // wait for server to start

        // test TCP proxy
        let mut conn = tokio::net::TcpStream::connect("127.0.0.1:54500")
            .await
            .unwrap();
        let mut buf = [0u8; 5];
        let _ = conn.write(b"hi").await.unwrap();
        let _ = conn.read(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");
        conn.shutdown().await.unwrap();

        // test TCP echo
        let mut conn = tokio::net::TcpStream::connect("127.0.0.1:54956")
            .await
            .unwrap();
        let mut buf = [0u8; 1];
        for i in 0..=10u8 {
            let _ = conn.write(&[i]).await.unwrap();
            let _ = conn.read(&mut buf).await.unwrap();
            assert_eq!(&buf, &[i]);
        }
        conn.shutdown().await.unwrap();
    }
}
