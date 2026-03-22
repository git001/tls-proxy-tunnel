mod proxy_to_upstream;

use crate::config::ViaUpstream;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use log::{debug, error};
use std::convert::Infallible;
use std::error::Error;
use std::fmt::Write as _;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

pub use crate::upstreams::proxy_to_upstream::ProxyToUpstream;

// ---------------------------------------------------------------------------
// Metrics — shared live view of per-proxy connection counts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MetricsEntry {
    pub name: String,
    pub listen: String,
    pub maxclients_limit: usize,
    /// Shared semaphore from the Proxy — available_permits() gives free slots.
    pub semaphore: Arc<Semaphore>,
}

/// Cheaply cloneable snapshot of all proxy metrics.
pub type Metrics = Arc<Vec<MetricsEntry>>;

// ---------------------------------------------------------------------------
// Upstream variants
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Upstream {
    Ban,
    Echo,
    /// Health + optional metrics for `/metrics` endpoint.
    /// Populated by `From<ParsedConfig> for Server` after all proxies are built.
    Health(Metrics),
    Proxy(ProxyToUpstream),
}

impl Upstream {
    pub(crate) async fn process(
        &self,
        mut inbound: TcpStream,
        via: &ViaUpstream,
        connect_target: Option<String>,
    ) -> Result<(), Box<dyn Error>> {
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
            Upstream::Health(metrics) => {
                let io = TokioIo::new(inbound);
                let metrics = metrics.clone();
                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |req| health_handler(req, metrics.clone())),
                        )
                        .await
                    {
                        error!("Error serving health connection: {:?}", err);
                    }
                });
            }
            Upstream::Proxy(config) => {
                config.proxy(inbound, via, connect_target).await?;
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
        Ok(n) => {
            let _ = writer.shutdown().await;
            Ok(n)
        }
        Err(e) => {
            let _ = writer.shutdown().await;
            error!("Copy issue {:?}", e);
            Ok(0)
        }
    }
}

async fn health_handler(
    req: Request<hyper::body::Incoming>,
    metrics: Metrics,
) -> Result<Response<Full<Bytes>>, Infallible> {
    match req.uri().path() {
        "/metrics" => {
            let mut body = String::new();
            writeln!(
                body,
                "# HELP tpt_active_connections Current number of active connections"
            )
            .unwrap();
            writeln!(body, "# TYPE tpt_active_connections gauge").unwrap();
            for e in metrics.iter() {
                let active = e
                    .maxclients_limit
                    .saturating_sub(e.semaphore.available_permits());
                writeln!(
                    body,
                    r#"tpt_active_connections{{name="{}",listen="{}"}} {}"#,
                    e.name, e.listen, active
                )
                .unwrap();
            }
            writeln!(
                body,
                "# HELP tpt_maxclients Maximum number of concurrent connections"
            )
            .unwrap();
            writeln!(body, "# TYPE tpt_maxclients gauge").unwrap();
            for e in metrics.iter() {
                writeln!(
                    body,
                    r#"tpt_maxclients{{name="{}",listen="{}"}} {}"#,
                    e.name, e.listen, e.maxclients_limit
                )
                .unwrap();
            }
            Ok(Response::builder()
                .header("Content-Type", "text/plain; version=0.0.4; charset=utf-8")
                .body(Full::new(Bytes::from(body)))
                .unwrap())
        }
        _ => Ok(Response::new(Full::new(Bytes::from("OK")))),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
