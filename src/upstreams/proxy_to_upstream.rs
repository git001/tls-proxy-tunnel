use crate::servers::upstream_address::UpstreamAddress;
use crate::servers::Proxy;
use crate::upstreams::copy;
use futures::future::try_join;
use log::{debug, error, info};
use serde::Deserialize;
use std::error::Error;
use std::fmt::{self};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{self};
use tokio::net::TcpStream;

#[derive(Debug)]
struct MyError(String);

impl fmt::Display for MyError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "There is an error: {}", self.0)
    }
}

impl Error for MyError {}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProxyToUpstream {
    pub addr: String,
    pub protocol: String,
    #[serde(skip_deserializing)]
    addresses: UpstreamAddress,
}

impl ProxyToUpstream {
    pub async fn resolve_addresses(&self) -> std::io::Result<Vec<SocketAddr>> {
        self.addresses.resolve((*self.protocol).into()).await
    }

    pub fn new(address: String, protocol: String) -> Self {
        Self {
            addr: address.clone(),
            protocol,
            addresses: UpstreamAddress::new(address),
        }
    }

    pub(crate) async fn proxy(
        &self,
        inbound: TcpStream,
        proxy: Arc<Proxy>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let outbound = match self.protocol.as_ref() {
            "tcp4" | "tcp6" | "tcp" => {
                TcpStream::connect(self.resolve_addresses().await?.as_slice()).await?
            }
            _ => {
                error!("Reached unknown protocol: {:?}", self.protocol);
                return Err("Reached unknown protocol".into());
            }
        };

        debug!("Connected to {:?}", outbound.peer_addr().unwrap());

        outbound.set_nodelay(true)?;
        inbound.set_nodelay(true)?;

        debug!("inbound {:?}", inbound);
        debug!("oubound {:?}", outbound);
        debug!("<<PROXY>> {:?}", proxy.via);

        loop {
            // Wait for the socket to be readable
            outbound.writable().await?;

            //let buf = String::from("CONNECT www.test1.com:4433 HTTP/1.1\r\n\r\n");
            /*
             * Build via upstream CONNECT sequence
             */
            let mut buf = String::with_capacity(256);
            buf.push_str("CONNECT ");
            buf.push_str(&proxy.via.target);
            buf.push_str(" HTTP/1.1\r\n");

            for (myeader, myvalue) in proxy.via.headers.clone() {
                debug!("Header name {:?}", myeader);
                debug!("Header valu {:?}", myvalue);
                buf.push_str(&myeader);
                buf.push_str(": ");
                buf.push_str(&myvalue);
                buf.push_str("\r\n");
            }

            buf.push_str("\r\n");

            //let buf = ("CONNECT {} HTTP/1.1\r\n\r\n",proxy.via.target);
            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.

            debug!("Send to via proxy :{:?}:", buf.as_str());

            match outbound.try_write(buf.as_bytes()) {
                Ok(0) => {
                    debug!("read returns 0");
                    break;
                }
                Ok(n) => {
                    debug!("written {} bytes", n);
                    debug!("written bufs {:?}", &buf);
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    //debug!("error {:?}", e);
                    continue;
                }
                Err(e) => {
                    debug!("any error {:?}", e);
                    return Err(e.into());
                }
            }
        }

        loop {
            outbound.readable().await?;

            // Creating the buffer **after** the `await` prevents it from
            // being stored in the async task.
            let mut inbufs = vec![0; 4096];
            //let decoder = LinesCodec::new();
            //let proxy_response = String::new();

            // Try to read data, this may still fail with `WouldBlock`
            // if the readiness event is a false positive.
            match outbound.try_read(&mut inbufs) {
                Ok(0) => {
                    debug!("read returns 0");
                    break;
                }
                Ok(n) => {
                    debug!("read :{:?}: bytes", n);
                    let mut i = 0 as u8;
                    for myiter in inbufs.split(|&x| x == b' ') {
                        i += 1;
                        debug!("myiter     :{:?}:", String::from_utf8(myiter.to_vec()));
                        debug!("myiter len :{:?}", myiter.len());

                        if i == 2 {
                            match String::from_utf8(myiter.to_vec()) {
                                Ok(proxy_response) => {
                                    debug!("proxy_response :{:?}", proxy_response);
                                    match proxy_response.as_str() {
                                        "200" => {
                                            debug!("Got 200 from Proxy");
                                        }
                                        "403" => {
                                            info!("Got: 403 ERR_ACCESS_DENIED. Proxy requires authentication.");
                                            return Err(Box::new(MyError(
                                                "Got: ERR_ACCESS_DENIED. Proxy requires authentication.".into(),
                                            )));
                                        }
                                        "503" => {
                                            info!("Got: 503 Service Unavailable.");
                                            return Err(Box::new(MyError(
                                                "Got: 503 Service Unavailable.".into(),
                                            )));
                                        }
                                        _ => {
                                            debug!("Got no 200,403 or 503 from Proxy");
                                            info!(
                                                "Proxy response {:?}",
                                                String::from_utf8(inbufs.to_vec()).unwrap()
                                            );
                                            return Err(Box::new(MyError(
                                                "Got no 200 from Proxy".into(),
                                            )));
                                        }
                                    }
                                }
                                Err(myerrproxy) => {
                                    debug!("proxy_response :{:?}", myerrproxy);
                                    return Err(myerrproxy.into());
                                }
                            }
                            debug!("myiter out");
                            break;
                        }
                    }
                    debug!("inbufs.split out");
                    break;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    continue;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }

        let (mut ri, mut wi) = io::split(inbound);
        let (mut ro, mut wo) = io::split(outbound);

        let inbound_to_outbound = copy(&mut ri, &mut wo);
        let outbound_to_inbound = copy(&mut ro, &mut wi);

        let (bytes_tx, bytes_rx) = try_join(inbound_to_outbound, outbound_to_inbound).await?;

        //info!("Connection closed to {:?}", inbound.peer_addr()?);
        info!(
            "Bytes read: {:?} write: {:?} for sni: {:?}",
            bytes_tx, bytes_rx, proxy.sni
        );

        Ok(())
    }
}
