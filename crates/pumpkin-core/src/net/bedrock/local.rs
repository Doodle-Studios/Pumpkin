//! An in-process [`Transport`] for the player who hosts the world.
//!
//! Limonite seam: the Limonite client runs this server inside its own process
//! and joins it as the host. Its bytes never touch a socket: they travel over a
//! pair of in-memory channels, and the server sees the host as one more
//! Bedrock connection behind the `Transport` trait.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use futures::future::BoxFuture;
use pumpkin_auth::p384::PublicKey;
use tokio::sync::{
    Mutex,
    mpsc::{self, error::TryRecvError, error::TrySendError},
};
use tokio_util::sync::CancellationToken;

use super::transport::Transport;

/// How many batches each direction holds before the sender has to wait.
///
/// Bounded on purpose: if the client stops reading, the server's outgoing task
/// blocks here and packets wait in Pumpkin's own queue, which is capped by
/// `MAX_PENDING_BYTES` exactly as for a slow `NetherNet` peer. An unbounded
/// channel would hide the stall and grow without limit.
pub const LOCAL_CHANNEL_CAPACITY: usize = 1024;

/// How many connections may wait for the accept loop at once.
const LISTENER_BACKLOG: usize = 16;

/// The server's end of an in-process connection.
pub struct LocalChannel {
    /// Batches for the client.
    to_client: mpsc::Sender<Bytes>,
    /// Batches from the client. Behind a mutex because `recv` takes `&self`.
    from_client: Mutex<mpsc::Receiver<Bytes>>,
    /// Cancelled by `close`; shared with the client end.
    closed: CancellationToken,
}

/// The client's end of an in-process connection.
///
/// Its methods never wait, so a plain thread with no async runtime (the
/// client's main loop) can drive it. Dropping it closes the connection.
pub struct LocalClientEnd {
    to_server: mpsc::Sender<Bytes>,
    from_server: mpsc::Receiver<Bytes>,
    closed: CancellationToken,
}

/// Builds a connected pair: the server's [`LocalChannel`] and the client's
/// [`LocalClientEnd`].
fn local_pair() -> (LocalChannel, LocalClientEnd) {
    let (to_client, from_server) = mpsc::channel(LOCAL_CHANNEL_CAPACITY);
    let (to_server, from_client) = mpsc::channel(LOCAL_CHANNEL_CAPACITY);
    let closed = CancellationToken::new();
    (
        LocalChannel {
            to_client,
            from_client: Mutex::new(from_client),
            closed: closed.clone(),
        },
        LocalClientEnd {
            to_server,
            from_server,
            closed,
        },
    )
}

impl Transport for LocalChannel {
    fn send(&self, batch: Bytes) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            // Waits while the client's queue is full; `close` wakes it.
            tokio::select! {
                biased;
                () = self.closed.cancelled() => Err("local connection is closed".to_string()),
                sent = self.to_client.send(batch) => {
                    sent.map_err(|_| "local client is gone".to_string())
                }
            }
        })
    }

    fn recv(&self) -> BoxFuture<'_, Option<Bytes>> {
        Box::pin(async move {
            let mut from_client = self.from_client.lock().await;
            // Racing the token lets `close` wake a pending `recv`. Without it,
            // the read loop would wait for bytes that never come, and server
            // shutdown would hang waiting for that task.
            tokio::select! {
                biased;
                () = self.closed.cancelled() => None,
                batch = from_client.recv() => batch,
            }
        })
    }

    fn flush_reliable(&self, _timeout: Duration) -> BoxFuture<'_, Result<(), String>> {
        // Nothing is ever in flight: a sent batch is already in the client's queue.
        Box::pin(async { Ok(()) })
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move { self.closed.cancel() })
    }

    fn is_closed(&self) -> bool {
        // `to_client` is closed once the client end has been dropped.
        self.closed.is_cancelled() || self.to_client.is_closed()
    }

    fn client_public_key(&self) -> Option<&PublicKey> {
        // The transport has no key of its own, so login accepts any well-formed `cpk`.
        None
    }
}

impl LocalClientEnd {
    /// Queues one batch for the server without waiting.
    ///
    /// # Errors
    /// `Full` if the server's queue is full, `Closed` if the server end is gone.
    pub fn try_send(&self, batch: Bytes) -> Result<(), TrySendError<Bytes>> {
        if self.closed.is_cancelled() {
            return Err(TrySendError::Closed(batch));
        }
        self.to_server.try_send(batch)
    }

    /// Takes the next batch from the server without waiting.
    ///
    /// Batches the server sent before closing are still delivered.
    ///
    /// # Errors
    /// `Empty` if nothing is queued, `Disconnected` if nothing ever will be.
    pub fn try_recv(&mut self) -> Result<Bytes, TryRecvError> {
        match self.from_server.try_recv() {
            Err(TryRecvError::Empty) if self.closed.is_cancelled() => {
                Err(TryRecvError::Disconnected)
            }
            other => other,
        }
    }

    /// Whether the server has closed the connection.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.is_cancelled() || self.to_server.is_closed()
    }
}

/// Accepts in-process connections. One per server, held by `PumpkinServer`.
pub struct LocalListener {
    /// Kept so the channel never closes: `accept` never returns `None`, which
    /// stops the accept loop's `select!` spinning on a closed channel.
    connector: LocalConnector,
    incoming: Mutex<mpsc::Receiver<(Arc<LocalChannel>, SocketAddr)>>,
}

/// Opens in-process connections to a [`LocalListener`]. Cheap to clone and
/// usable from any thread.
#[derive(Clone)]
pub struct LocalConnector {
    sender: mpsc::Sender<(Arc<LocalChannel>, SocketAddr)>,
    next_connection: Arc<AtomicU16>,
}

impl LocalListener {
    #[must_use]
    pub fn new() -> Self {
        let (sender, incoming) = mpsc::channel(LISTENER_BACKLOG);
        Self {
            connector: LocalConnector {
                sender,
                next_connection: Arc::new(AtomicU16::new(1)),
            },
            incoming: Mutex::new(incoming),
        }
    }

    /// A handle for opening connections to this listener.
    #[must_use]
    pub fn connector(&self) -> LocalConnector {
        self.connector.clone()
    }

    /// Waits for the next in-process connection.
    pub async fn accept(&self) -> Option<(Arc<LocalChannel>, SocketAddr)> {
        self.incoming.lock().await.recv().await
    }
}

impl Default for LocalListener {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalConnector {
    /// Opens a connection and returns the client's end of it.
    ///
    /// The address is `127.0.0.1:<connection number>`. It's only a key in the
    /// server's client table; nothing is bound.
    ///
    /// # Errors
    /// `WouldBlock` if too many connections are waiting to be accepted.
    pub fn connect(&self) -> io::Result<LocalClientEnd> {
        let (channel, client) = local_pair();
        let number = self.next_connection.fetch_add(1, Ordering::Relaxed);
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, number));
        self.sender
            .try_send((Arc::new(channel), addr))
            .map_err(|err| match err {
                TrySendError::Full(_) => io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "too many local connections waiting to be accepted",
                ),
                TrySendError::Closed(_) => {
                    io::Error::new(io::ErrorKind::NotConnected, "the local listener is gone")
                }
            })?;
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Long enough for a correct test, short enough that a hang fails fast.
    const WAIT: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn both_directions_deliver_in_order() {
        let (channel, mut client) = local_pair();

        for byte in 1..=3u8 {
            channel.send(Bytes::from(vec![byte])).await.unwrap();
        }
        for byte in 1..=3u8 {
            assert_eq!(client.try_recv().unwrap(), Bytes::from(vec![byte]));
        }
        assert_eq!(client.try_recv(), Err(TryRecvError::Empty));

        for byte in 4..=6u8 {
            client.try_send(Bytes::from(vec![byte])).unwrap();
        }
        for byte in 4..=6u8 {
            assert_eq!(channel.recv().await, Some(Bytes::from(vec![byte])));
        }
    }

    #[tokio::test]
    async fn close_wakes_a_pending_recv() {
        let (channel, _client) = local_pair();
        let channel = Arc::new(channel);
        let reader = channel.clone();
        let pending = tokio::spawn(async move { reader.recv().await });
        // Let the reader start waiting before closing.
        tokio::task::yield_now().await;

        channel.close().await;

        let received = tokio::time::timeout(WAIT, pending).await.unwrap().unwrap();
        assert_eq!(received, None);
        assert!(channel.is_closed());
    }

    #[tokio::test]
    async fn close_wakes_a_send_waiting_on_a_full_queue() {
        let (channel, _client) = local_pair();
        for _ in 0..LOCAL_CHANNEL_CAPACITY {
            channel.send(Bytes::new()).await.unwrap();
        }
        let channel = Arc::new(channel);
        let sender = channel.clone();
        let pending = tokio::spawn(async move { sender.send(Bytes::new()).await });
        tokio::task::yield_now().await;

        channel.close().await;

        let sent = tokio::time::timeout(WAIT, pending).await.unwrap().unwrap();
        assert!(sent.is_err());
    }

    #[tokio::test]
    async fn dropping_the_client_end_closes_the_channel() {
        let (channel, client) = local_pair();
        drop(client);

        assert!(channel.is_closed());
        assert_eq!(channel.recv().await, None);
        assert!(channel.send(Bytes::new()).await.is_err());
    }

    #[tokio::test]
    async fn the_client_sees_the_server_close_after_draining() {
        let (channel, mut client) = local_pair();
        channel.send(Bytes::from_static(b"bye")).await.unwrap();
        channel.close().await;

        assert!(client.is_closed());
        assert_eq!(client.try_recv().unwrap(), Bytes::from_static(b"bye"));
        assert_eq!(client.try_recv(), Err(TryRecvError::Disconnected));
        assert!(client.try_send(Bytes::new()).is_err());
    }

    #[test]
    fn has_no_client_public_key() {
        let (channel, _client) = local_pair();
        assert!(channel.client_public_key().is_none());
    }

    #[tokio::test]
    async fn the_listener_accepts_what_the_connector_opens() {
        let listener = LocalListener::new();
        let connector = listener.connector();

        let mut first = connector.connect().unwrap();
        let _second = connector.connect().unwrap();

        let (channel, addr) = tokio::time::timeout(WAIT, listener.accept())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(addr, SocketAddr::from((Ipv4Addr::LOCALHOST, 1)));
        let (_, second_addr) = listener.accept().await.unwrap();
        assert_eq!(second_addr.port(), 2);

        channel.send(Bytes::from_static(b"hi")).await.unwrap();
        assert_eq!(first.try_recv().unwrap(), Bytes::from_static(b"hi"));
    }
}
