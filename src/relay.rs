//! Byte relay between two connections.

use std::sync::Arc;
use std::thread;

use networkframework::{NetworkError, TcpClient};

/// Largest chunk moved per receive.
pub const CHUNK: usize = 65536;

/// Copy `from` → `to` until `from` ends or fails, then cancel both
/// connections gracefully (TCP FIN, not a reset). Cancelling also
/// unblocks the opposite direction's receive.
pub fn pipe(from: &TcpClient, to: &TcpClient) {
    while let Ok(data) = from.receive(CHUNK) {
        if data.is_empty() || to.send(&data).is_err() {
            break;
        }
    }
    from.cancel();
    to.cancel();
}

/// Relay `inbound` to the TCP service at `host:port` (sshd) until either side
/// ends — what `airshell-sshd` does with every accepted connection. Blocks for the
/// life of the relay: one direction runs on a helper thread, the other here.
pub fn relay_to(inbound: TcpClient, host: &str, port: u16) -> Result<(), NetworkError> {
    let outbound = match TcpClient::connect(host, port) {
        Ok(outbound) => Arc::new(outbound),
        Err(error) => {
            inbound.cancel();
            return Err(error);
        }
    };
    let inbound = Arc::new(inbound);
    let (from, to) = (Arc::clone(&inbound), Arc::clone(&outbound));
    let upstream = thread::spawn(move || pipe(&from, &to));
    pipe(&outbound, &inbound);
    let _ = upstream.join();
    Ok(())
}
