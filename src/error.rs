//! Error type for airshell relays.

use networkframework::NetworkError;

/// What can go wrong while relaying a peer-to-peer connection to a local service.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Could not reach the local service the daemon relays to (e.g. sshd is off).
    #[error("cannot reach local service at {host}:{port}: {source}")]
    Upstream {
        host: String,
        port: u16,
        #[source]
        source: NetworkError,
    },

    /// A peer-to-peer (AWDL) networking error.
    #[error(transparent)]
    Network(#[from] NetworkError),
}

/// Convenience alias for airshell results.
pub type Result<T> = std::result::Result<T, Error>;
