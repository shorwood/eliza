//! Socket deadlines cover backpressure even when Hyper stops polling the body.

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::extract::connect_info::Connected;
use axum::serve::{IncomingStream, Listener};
use eliza_http::hosted::HostedConfig;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::Instant;
use tokio_io_timeout::TimeoutStream;
use tokio_util::sync::{CancellationToken, WaitForCancellationFutureOwned};

// -----------------------------------------------------------------------------
// ConnectionBounds: Names hosted socket idle and absolute lifetimes.
// -----------------------------------------------------------------------------

/// Optional hosted socket bounds; local mode preserves unbounded connections.
#[derive(Clone, Copy)]
struct ConnectionBounds {
    /// Maximum pending socket write duration.
    idle: Duration,
}

// -----------------------------------------------------------------------------
// Connection: Enforces deadlines below HTTP body backpressure.
// -----------------------------------------------------------------------------

/// Origin socket retaining idle and absolute deadlines independent of body polling.
pub(super) struct Connection {
    /// Write timeout wrapper around the actual TCP socket.
    stream: Pin<Box<TimeoutStream<TcpStream>>>,
    /// Shared interruption signal set only by an expired active response.
    shutdown: CancellationToken,
    /// Registers IO wakeups independently of body polling and write backpressure.
    canceled: Pin<Box<WaitForCancellationFutureOwned>>,
}

impl Connection {
    /// Interrupt expired active responses even when writes continue making occasional progress.
    ///
    /// # Errors
    /// Returns a timeout after an admitted response's absolute lifetime expires.
    fn check_deadline(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        // Active-response watchdogs wake stalled IO without shortening healthy keepalive reuse.
        if self.canceled.as_mut().poll(cx).is_ready() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(())
    }
}

impl AsyncRead for Connection {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.check_deadline(cx)?;
        self.stream.as_mut().poll_read(cx, buffer)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.check_deadline(cx)?;
        self.stream.as_mut().poll_write(cx, bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.check_deadline(cx)?;
        self.stream.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.as_mut().poll_shutdown(cx)
    }
}

// -----------------------------------------------------------------------------
// OriginListener: Wraps sockets while preserving peer identity.
// -----------------------------------------------------------------------------

/// Listener that preserves local behavior and applies explicit hosted socket policy.
pub(super) struct OriginListener {
    /// Actual origin listener.
    inner: TcpListener,
    /// Validated hosted limits, absent in local mode.
    bounds: Option<ConnectionBounds>,
}

impl OriginListener {
    /// Capture nonsecret timeout policy for every accepted socket.
    pub(super) fn new(inner: TcpListener, config: Option<&HostedConfig>) -> Self {
        let bounds = config.map(|config| ConnectionBounds {
            idle: Duration::from_secs(config.idle_seconds),
        });
        Self { inner, bounds }
    }
}

impl Listener for OriginListener {
    type Io = Connection;

    type Addr = SocketAddr;

    #[expect(
        rlib::bare_tuple_types,
        reason = "Axum Listener mandates the accepted IO and peer address tuple"
    )]
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (socket, address) = <TcpListener as Listener>::accept(&mut self.inner).await;
        let mut stream = TimeoutStream::new(socket);
        if let Some(bounds) = self.bounds {
            stream.set_write_timeout(Some(bounds.idle));
        }
        let shutdown = CancellationToken::new();
        let canceled = Box::pin(shutdown.clone().cancelled_owned());
        (
            Connection {
                stream: Box::pin(stream),
                shutdown,
                canceled,
            },
            address,
        )
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

// -----------------------------------------------------------------------------
// Peer: Retains the actual immediate socket identity.
// -----------------------------------------------------------------------------

/// Actual TCP peer and interruption capability, independent of forwarding headers.
#[derive(Clone)]
pub(super) struct Peer {
    /// Actual remote TCP address.
    pub(super) address: SocketAddr,
    /// Shared IO interruption capability for an expired active response.
    shutdown: CancellationToken,
}

impl Peer {
    /// Bound an admitted response without expiring healthy pooled connections.
    pub(super) fn deadline(&self, until: Instant) -> ResponseDeadline {
        let shutdown = self.shutdown.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep_until(until).await;
            shutdown.cancel();
        });
        ResponseDeadline { task }
    }
}

#[cfg(test)]
impl From<SocketAddr> for Peer {
    fn from(address: SocketAddr) -> Self {
        Self {
            address,
            shutdown: CancellationToken::new(),
        }
    }
}

impl Connected<IncomingStream<'_, OriginListener>> for Peer {
    fn connect_info(stream: IncomingStream<'_, OriginListener>) -> Self {
        Self {
            address: *stream.remote_addr(),
            shutdown: stream.io().shutdown.clone(),
        }
    }
}

// -----------------------------------------------------------------------------
// ResponseDeadline: Owns one watchdog per admitted response.
// -----------------------------------------------------------------------------

/// Drop cancels the watchdog; expiration interrupts the socket even under backpressure.
pub(super) struct ResponseDeadline {
    /// At most one watchdog exists for each owned response permit.
    task: tokio::task::JoinHandle<()>,
}

impl Drop for ResponseDeadline {
    fn drop(&mut self) {
        self.task.abort();
    }
}
