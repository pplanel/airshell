//! airshell: SSH over AWDL via Network.framework (through the `networkframework` crate).

pub mod relay;
pub mod transport;

use networkframework::{ConnectionParameters, NetworkError};

/// Bonjour service type that airshell-sshd advertises and airshell-connect looks up.
pub const SERVICE_TYPE: &str = "_awdlssh._tcp";

/// Plain TCP (no TLS) with peer-to-peer interfaces (AWDL) allowed.
pub fn peer_to_peer_tcp() -> Result<ConnectionParameters, NetworkError> {
    let mut parameters = ConnectionParameters::tcp()?;
    parameters.set_include_peer_to_peer(true);
    Ok(parameters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Network.framework: run on a test Mac via scripts/remote-test.sh <host>"]
    fn peer_to_peer_tcp_allows_awdl() {
        assert!(peer_to_peer_tcp().unwrap().include_peer_to_peer());
    }
}
