use futures::future::try_join;
use log::{error, info};
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Bidirectional relay with optional periodic rx/tx stats logging.
//
// Both streams are consumed. Returns (bytes_tx, bytes_rx) where:
//   bytes_tx = inbound → outbound
//   bytes_rx = outbound → inbound
//
// If `stats_interval` is non-zero a background task logs the running counters
// at that interval until the relay completes.
// ---------------------------------------------------------------------------
pub(super) async fn relay(
    inbound: TcpStream,
    outbound: TcpStream,
    label: String,
    stats_interval: Duration,
) -> Result<(u64, u64), Box<dyn Error>> {
    let bytes_tx = Arc::new(AtomicU64::new(0));
    let bytes_rx = Arc::new(AtomicU64::new(0));

    let (mut ri, mut wi) = io::split(inbound);
    let (mut ro, mut wo) = io::split(outbound);

    // Spawn periodic stats logger if configured -------------------------------
    let log_handle = if stats_interval > Duration::ZERO {
        let tx = bytes_tx.clone();
        let rx = bytes_rx.clone();
        Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(stats_interval);
            ticker.tick().await; // skip the first immediate tick
            loop {
                ticker.tick().await;
                info!(
                    "[relay:{}] in-flight tx={} rx={}",
                    label,
                    tx.load(Ordering::Relaxed),
                    rx.load(Ordering::Relaxed),
                );
            }
        }))
    } else {
        None
    };

    let result = try_join(
        copy_counted(&mut ri, &mut wo, bytes_tx.clone()),
        copy_counted(&mut ro, &mut wi, bytes_rx.clone()),
    )
    .await;

    if let Some(h) = log_handle {
        h.abort();
    }

    let (tx, rx) = result?;
    Ok((tx, rx))
}

// ---------------------------------------------------------------------------
// Copy bytes from reader to writer, updating an atomic counter as we go.
// Shuts down the writer when the reader closes or errors.
// ---------------------------------------------------------------------------
async fn copy_counted(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    counter: Arc<AtomicU64>,
) -> io::Result<u64> {
    let mut buf = vec![0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let _ = writer.shutdown().await;
                error!("Copy read error: {:?}", e);
                return Ok(total);
            }
        };
        if let Err(e) = writer.write_all(&buf[..n]).await {
            let _ = writer.shutdown().await;
            error!("Copy write error: {:?}", e);
            return Ok(total);
        }
        total += n as u64;
        counter.fetch_add(n as u64, Ordering::Relaxed);
    }
    let _ = writer.shutdown().await;
    Ok(total)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
