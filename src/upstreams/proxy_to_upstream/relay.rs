use futures::future::try_join;
use log::{error, info};
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

// ---------------------------------------------------------------------------
// Time of the last data in either direction, for the idle timeout.
// ---------------------------------------------------------------------------
pub(super) struct Activity {
    start: Instant,
    /// Milliseconds since `start` at the last read.
    last_ms: AtomicU64,
}

impl Activity {
    pub(super) fn new() -> Self {
        Self {
            start: Instant::now(),
            last_ms: AtomicU64::new(0),
        }
    }

    fn touch(&self) {
        let now = self.start.elapsed().as_millis() as u64;
        self.last_ms.store(now, Ordering::Relaxed);
    }

    fn idle_for(&self) -> Duration {
        let last = Duration::from_millis(self.last_ms.load(Ordering::Relaxed));
        self.start.elapsed().saturating_sub(last)
    }

    /// Completes once nothing was read in either direction for `timeout`.
    async fn idle(&self, timeout: Duration) {
        loop {
            let idle = self.idle_for();
            if idle >= timeout {
                return;
            }
            tokio::time::sleep(timeout - idle).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Bidirectional relay with optional periodic rx/tx stats logging and an
// optional idle timeout.
//
// Both streams are consumed. Returns (bytes_tx, bytes_rx) where:
//   bytes_tx = inbound → outbound
//   bytes_rx = outbound → inbound
//
// If `stats_interval` is non-zero a background task logs the running counters
// at that interval until the relay completes. If `idle_timeout` is non-zero
// the relay ends (both connections close) after no data flowed in either
// direction for that long.
// ---------------------------------------------------------------------------
pub(super) async fn relay(
    inbound: TcpStream,
    outbound: TcpStream,
    label: String,
    stats_interval: Duration,
    idle_timeout: Duration,
) -> Result<(u64, u64), Box<dyn Error>> {
    let bytes_tx = Arc::new(AtomicU64::new(0));
    let bytes_rx = Arc::new(AtomicU64::new(0));
    let activity = Activity::new();

    // Owned halves: no lock between the two directions (io::split has one).
    let (mut ri, mut wi) = inbound.into_split();
    let (mut ro, mut wo) = outbound.into_split();

    // Spawn periodic stats logger if configured -------------------------------
    let log_handle = if stats_interval > Duration::ZERO {
        let tx = bytes_tx.clone();
        let rx = bytes_rx.clone();
        let label = label.clone();
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

    let copy = try_join(
        copy_counted(&mut ri, &mut wo, &bytes_tx, &activity),
        copy_counted(&mut ro, &mut wi, &bytes_rx, &activity),
    );
    let result = if idle_timeout.is_zero() {
        copy.await
    } else {
        tokio::select! {
            result = copy => result,
            _ = activity.idle(idle_timeout) => {
                // Dropping the halves below closes both connections.
                info!("[relay:{}] idle for {:?}, closing", label, idle_timeout);
                Ok((
                    bytes_tx.load(Ordering::Relaxed),
                    bytes_rx.load(Ordering::Relaxed),
                ))
            }
        }
    };

    if let Some(h) = log_handle {
        h.abort();
    }

    let (tx, rx) = result?;
    Ok((tx, rx))
}

// ---------------------------------------------------------------------------
// Copy bytes from reader to writer, updating an atomic counter and the
// activity time as we go. Shuts down the writer when the reader closes or
// errors.
// ---------------------------------------------------------------------------
pub(super) async fn copy_counted(
    reader: &mut (impl AsyncRead + Unpin),
    writer: &mut (impl AsyncWrite + Unpin),
    counter: &AtomicU64,
    activity: &Activity,
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
        activity.touch();
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
