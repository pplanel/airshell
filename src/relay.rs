//! Byte relay between two connections.

use std::thread;

use networkframework::{NetworkError, TcpClient};

use crate::transport::Transport;

/// Largest chunk moved per receive.
pub const CHUNK: usize = 65536;

/// Copy `from` → `to` until `from` ends or fails, then cancel both
/// connections gracefully (TCP FIN, not a reset). Cancelling also
/// unblocks the opposite direction's receive.
pub fn pipe(from: &impl Transport, to: &impl Transport) {
    while let Ok(data) = from.receive(CHUNK) {
        if data.is_empty() || to.send(&data).is_err() {
            break;
        }
    }
    from.cancel();
    to.cancel();
}

/// Relay `a` ↔ `b` until either side ends. Blocks for the life of the
/// relay: one direction runs on a helper thread, the other here.
pub fn relay(a: &impl Transport, b: &impl Transport) {
    thread::scope(|scope| {
        scope.spawn(|| pipe(a, b));
        pipe(b, a);
    });
}

/// Relay `inbound` to the TCP service at `host:port` (sshd) until either side
/// ends — what `airshell-sshd` does with every accepted connection.
pub fn relay_to(inbound: impl Transport, host: &str, port: u16) -> Result<(), NetworkError> {
    let outbound = match TcpClient::connect(host, port) {
        Ok(outbound) => outbound,
        Err(error) => {
            inbound.cancel();
            return Err(error);
        }
    };
    relay(&inbound, &outbound);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::Duration;

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(5);

    /// One direction of an in-memory connection.
    #[derive(Default)]
    struct Channel {
        state: Mutex<State>,
        changed: Condvar,
    }

    #[derive(Default)]
    struct State {
        data: VecDeque<u8>,
        closed: bool,
    }

    impl Channel {
        fn close(&self) {
            self.state.lock().unwrap().closed = true;
            self.changed.notify_all();
        }
    }

    /// One end of an in-memory connection. `cancel` closes both directions,
    /// so the peer sees EOF and a `receive` blocked on this end returns.
    struct End {
        inbox: Arc<Channel>,
        outbox: Arc<Channel>,
    }

    fn connected() -> (End, End) {
        let (x, y) = (Arc::new(Channel::default()), Arc::new(Channel::default()));
        let a = End {
            inbox: Arc::clone(&x),
            outbox: Arc::clone(&y),
        };
        (
            a,
            End {
                inbox: y,
                outbox: x,
            },
        )
    }

    impl Transport for End {
        type Error = ();

        fn receive(&self, max_len: usize) -> Result<Vec<u8>, ()> {
            let (mut state, wait) = self
                .inbox
                .changed
                .wait_timeout_while(self.inbox.state.lock().unwrap(), TIMEOUT, |s| {
                    s.data.is_empty() && !s.closed
                })
                .unwrap();
            assert!(!wait.timed_out(), "receive blocked for {TIMEOUT:?}");
            let n = state.data.len().min(max_len);
            Ok(state.data.drain(..n).collect())
        }

        fn send(&self, data: &[u8]) -> Result<(), ()> {
            let mut state = self.outbox.state.lock().unwrap();
            if state.closed {
                return Err(());
            }
            state.data.extend(data);
            self.outbox.changed.notify_all();
            Ok(())
        }

        fn cancel(&self) {
            self.inbox.close();
            self.outbox.close();
        }
    }

    #[test]
    fn relays_both_ways_and_tears_down_when_either_side_ends() {
        for client_ends in [true, false] {
            let (client, inbound) = connected();
            let (outbound, server) = connected();
            let relaying = thread::spawn(move || relay(&inbound, &outbound));

            client.send(b"ping").unwrap();
            assert_eq!(server.receive(CHUNK).unwrap(), b"ping");
            server.send(b"pong").unwrap();
            assert_eq!(client.receive(CHUNK).unwrap(), b"pong");

            let (closer, other) = if client_ends {
                (client, server)
            } else {
                (server, client)
            };
            closer.cancel();
            assert_eq!(other.receive(CHUNK).unwrap(), b"", "expected EOF");
            relaying.join().unwrap();
        }
    }
}
