//! Byte relay between two connections.

use std::sync::Arc;
use std::thread;

use networkframework::TcpClient;

use crate::error::{Error, Result};

/// Largest chunk moved per receive.
pub const CHUNK: usize = 65536;

/// Copy `from` → `to` until `from` ends or fails, then cancel both
/// connections gracefully (TCP FIN, not a reset). Cancelling also
/// unblocks the opposite direction's receive. Returns the number of
/// bytes copied.
pub fn pipe(from: &TcpClient, to: &TcpClient) -> u64 {
    let mut moved = 0u64;
    while let Ok(data) = from.receive(CHUNK) {
        if data.is_empty() || to.send(&data).is_err() {
            break;
        }
        moved += data.len() as u64;
    }
    from.cancel();
    to.cancel();
    moved
}

/// Bytes copied in each direction over the life of a relay.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Transferred {
    /// Bytes from the peer to the local service.
    pub to_service: u64,
    /// Bytes from the local service back to the peer.
    pub to_peer: u64,
}

/// Relay `inbound` to the TCP service at `host:port` (sshd) until either side
/// ends — what `airshell-proxy` does with every accepted connection. Blocks for
/// the life of the relay: one direction runs on a helper thread, the other here.
/// Returns how many bytes moved each way.
pub fn relay_to(inbound: TcpClient, host: &str, port: u16) -> Result<Transferred> {
    let outbound = match TcpClient::connect(host, port) {
        Ok(outbound) => Arc::new(outbound),
        Err(source) => {
            inbound.cancel();
            return Err(Error::Upstream {
                host: host.to_owned(),
                port,
                source,
            });
        }
    };
    let inbound = Arc::new(inbound);
    let (from, to) = (Arc::clone(&inbound), Arc::clone(&outbound));
    let upstream = thread::spawn(move || pipe(&from, &to));
    let to_peer = pipe(&outbound, &inbound);
    let to_service = upstream.join().unwrap_or(0);
    Ok(Transferred {
        to_service,
        to_peer,
    })
}
