use log::{debug, error, info};
use std::collections::HashMap;
use std::error::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::ProxyError;

/// Largest CONNECT response header accepted.
const MAX_RESPONSE_HEADER: usize = 16 * 1024;

// ---------------------------------------------------------------------------
// Send an HTTP CONNECT request and verify the upstream returns 2xx.
// Returns the bytes the proxy sent after the response header: data from a
// target that speaks first can arrive in the same read.
// ---------------------------------------------------------------------------
pub(super) async fn http_connect(
    outbound: &mut TcpStream,
    target: &str,
    headers: &HashMap<String, String>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    // Build request -----------------------------------------------------------
    let mut buf = String::with_capacity(256);
    buf.push_str("CONNECT ");
    buf.push_str(target);
    buf.push_str(" HTTP/1.1\r\n");

    for (name, value) in headers {
        buf.push_str(name);
        buf.push_str(": ");
        buf.push_str(&resolve_header_value(value)?);
        buf.push_str("\r\n");
    }
    buf.push_str("\r\n");

    debug!("Send to via proxy: {:?}", buf.as_str());
    outbound.write_all(buf.as_bytes()).await?;

    // Read until the end of the response header (\r\n\r\n) -----------------
    let mut inbufs = vec![0u8; MAX_RESPONSE_HEADER];
    let mut total = 0;
    let header_end = loop {
        if total == inbufs.len() {
            return Err(Box::new(ProxyError(
                "CONNECT response header too large".into(),
            )));
        }
        let n = outbound.read(&mut inbufs[total..]).await?;
        if n == 0 {
            return Err(Box::new(ProxyError(
                "upstream proxy closed connection before sending CONNECT response".into(),
            )));
        }
        total += n;
        debug!("read {} bytes ({} total)", n, total);
        if let Some(pos) = inbufs[..total].windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let status = parse_connect_status(&inbufs[..header_end])?;
    debug!("CONNECT response status: {}", status);
    match status {
        200..=299 => Ok(inbufs[header_end..total].to_vec()),
        403 => {
            info!("Got: 403 ERR_ACCESS_DENIED. Proxy requires authentication.");
            Err(Box::new(ProxyError(
                "Got: ERR_ACCESS_DENIED. Proxy requires authentication.".into(),
            )))
        }
        502 => {
            info!("Got: 502 Bad Gateway.");
            Err(Box::new(ProxyError("Got: 502 Bad Gateway.".into())))
        }
        503 => {
            info!("Got: 503 Service Unavailable.");
            Err(Box::new(ProxyError("Got: 503 Service Unavailable.".into())))
        }
        other => {
            info!(
                "Unexpected proxy response {}: {}",
                other,
                String::from_utf8_lossy(&inbufs[..header_end])
            );
            Err(Box::new(ProxyError(format!(
                "upstream proxy returned status {}",
                other
            ))))
        }
    }
}

/// Parse the HTTP status code from a CONNECT response (e.g. "HTTP/1.1 200 Connection established").
fn parse_connect_status(buf: &[u8]) -> Result<u16, Box<dyn Error>> {
    let text = String::from_utf8_lossy(buf);
    let status_str = text
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| ProxyError("malformed CONNECT response: no status code".into()))?;
    status_str.parse::<u16>().map_err(|e| {
        ProxyError(format!(
            "malformed CONNECT response status '{}': {}",
            status_str, e
        ))
        .into()
    })
}

/// Resolve `$VARNAME` placeholders in a header value from environment variables.
/// The variable name ends at the first character that is not alphanumeric or `_`.
pub(super) fn resolve_header_value(value: &str) -> Result<String, Box<dyn Error>> {
    let Some(start) = value.find('$') else {
        return Ok(value.to_string());
    };
    let after_dollar = &value[start + 1..];
    let name_len = after_dollar
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(after_dollar.len());
    if name_len == 0 {
        return Ok(value.to_string());
    }
    let var_name = &after_dollar[..name_len];
    let prefix = &value[..start];
    let suffix = &after_dollar[name_len..];
    match std::env::var(var_name) {
        Ok(v) => {
            debug!("Env var {:?} resolved", var_name);
            Ok(format!("{}{}{}", prefix, v, suffix))
        }
        Err(e) => {
            error!("couldn't find env var {:?}", var_name);
            Err(e.into())
        }
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
