use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use pumpkin_auth::p384::PublicKey;

use super::nethernet::NetherNetSession;

/// The one place a Bedrock player's bytes enter and leave the server.
///
/// Limonite seam: exists so a player can be connected by `NetherNet` (LAN or
/// remote peers) or by an in-process `LocalChannel` (the host) without game
/// code knowing which. Method names match `NetherNetSession`'s on purpose,
/// so existing call sites compile unchanged.
///
/// The async methods return [`BoxFuture`] rather than being `async fn`, because
/// `BedrockClient` holds an `Arc<dyn Transport>` and an `async fn` in a trait
/// can't be called through `dyn`.
pub trait Transport: Send + Sync {
    /// Sends one complete batch on the reliable, ordered path.
    fn send(&self, batch: Bytes) -> BoxFuture<'_, Result<(), String>>;
    /// Waits for the next complete batch. `None` means the connection closed.
    fn recv(&self) -> BoxFuture<'_, Option<Bytes>>;
    /// Waits until everything sent so far has been delivered, or `timeout` passes.
    fn flush_reliable(&self, timeout: Duration) -> BoxFuture<'_, Result<(), String>>;
    /// Closes the connection. Closing twice is a no-op.
    fn close(&self) -> BoxFuture<'_, ()>;
    fn is_closed(&self) -> bool;
    /// The key the transport itself verified the client with, if any. Login
    /// checks it against the `cpk` claim in the client's token.
    fn client_public_key(&self) -> Option<&PublicKey>;
}

// `Self::send` etc. name `NetherNetSession`'s own (inherent) methods: Rust
// prefers inherent methods over trait methods with the same name, so these
// don't recurse into the trait.
impl Transport for NetherNetSession {
    fn send(&self, batch: Bytes) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(Self::send(self, batch))
    }

    fn recv(&self) -> BoxFuture<'_, Option<Bytes>> {
        Box::pin(Self::recv(self))
    }

    fn flush_reliable(&self, timeout: Duration) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(Self::flush_reliable(self, timeout))
    }

    fn close(&self) -> BoxFuture<'_, ()> {
        Box::pin(Self::close(self))
    }

    fn is_closed(&self) -> bool {
        Self::is_closed(self)
    }

    fn client_public_key(&self) -> Option<&PublicKey> {
        Self::client_public_key(self)
    }
}
