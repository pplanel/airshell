//! The connection operations the relay needs, independent of how the
//! connection was made (Network.framework TCP over AWDL today).

use networkframework::{NetworkError, TcpClient};

/// A connected byte stream shared across threads: one thread receives while
/// another sends, and either may cancel.
pub trait Transport: Sync {
    type Error;

    /// Block until at least one byte (at most `max_len`) arrives. Empty
    /// means the peer has finished sending.
    fn receive(&self, max_len: usize) -> Result<Vec<u8>, Self::Error>;

    /// Send all of `data`.
    fn send(&self, data: &[u8]) -> Result<(), Self::Error>;

    /// Close gracefully. Must unblock a `receive` waiting on another thread,
    /// and be idempotent.
    fn cancel(&self);
}

impl Transport for TcpClient {
    type Error = NetworkError;

    fn receive(&self, max_len: usize) -> Result<Vec<u8>, NetworkError> {
        TcpClient::receive(self, max_len)
    }

    fn send(&self, data: &[u8]) -> Result<(), NetworkError> {
        TcpClient::send(self, data)
    }

    fn cancel(&self) {
        TcpClient::cancel(self);
    }
}
