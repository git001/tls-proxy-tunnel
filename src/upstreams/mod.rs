mod proxy_to_upstream;

use crate::servers::Proxy;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use hyper::{Request, Response};
use http_body_util::Full;
use hyper::body::Bytes;
use log::debug;
use serde::Deserialize;
use std::error::Error;
use std::sync::Arc;
use tokio::io;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use std::convert::Infallible;


pub use crate::upstreams::proxy_to_upstream::ProxyToUpstream;

#[derive(Debug, Clone, Deserialize)]
pub enum Upstream {
    Ban,
    Echo,
    Health,
    Proxy(ProxyToUpstream),
}

impl Upstream {
    pub(crate) async fn process(
        &self,
        mut inbound: TcpStream,
        proxy: Arc<Proxy>,
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
            Upstream::Health => {
                // Use an adapter to access something implementing `tokio::io` traits as if they implement
                // `hyper::rt` IO traits.
                let io = TokioIo::new(inbound);

                // Spawn a tokio task to serve multiple connections concurrently
                tokio::task::spawn(async move {
                    // Finally, we bind the incoming connection to our `hello` service
                    if let Err(err) = http1::Builder::new()
                        // `service_fn` converts our function in a `Service`
                        .serve_connection(io, service_fn(health))
                        .await
                    {
                        eprintln!("Error serving connection: {:?}", err);
                    }
                });
            }
            Upstream::Proxy(config) => {
                debug!("Process proxy {:?}", proxy);
                config.proxy(inbound, proxy.clone()).await?;
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
        Ok(u64) => {
            let _ = writer.shutdown().await;
            Ok(u64)
        }
        Err(_) => Ok(0),
    }
}

async fn health(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from("OK"))))
}
