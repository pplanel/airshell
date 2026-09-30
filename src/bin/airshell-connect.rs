//! ssh ProxyCommand: connects to a peer's airshell-sshd by Bonjour name and bridges stdin/stdout.

use std::io::{ErrorKind, Read, Write};
use std::process::exit;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use airshell::relay::CHUNK;
use airshell::{SERVICE_TYPE, peer_to_peer_tcp};
use networkframework::{ContentContext, Endpoint, NetworkError, TcpClient};

/// Stop retrying after this long.
const CONNECT_DEADLINE: Duration = Duration::from_secs(30);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("usage: airshell-connect <ServiceName>");
        exit(2);
    }
    let conn = match connect(&args[1]) {
        Ok(conn) => Arc::new(conn),
        Err(error) => {
            eprintln!("airshell-connect: {error}");
            exit(1);
        }
    };

    let sender = Arc::clone(&conn);
    thread::spawn(move || forward_stdin(&sender));

    let mut stdout = std::io::stdout().lock();
    while let Ok(data) = conn.receive(CHUNK) {
        if data.is_empty()
            || stdout
                .write_all(&data)
                .and_then(|()| stdout.flush())
                .is_err()
        {
            break;
        }
    }
    exit(0);
}

/// Connect to `<service_name>._awdlssh._tcp.local.` with peer-to-peer enabled.
/// The crate fails a connect on its first `waiting` error, so report each
/// failure and retry until the deadline.
fn connect(service_name: &str) -> Result<TcpClient, NetworkError> {
    let parameters = peer_to_peer_tcp()?;
    let endpoint = Endpoint::bonjour_service(Some(service_name), SERVICE_TYPE, Some("local."))?;
    let deadline = Instant::now() + CONNECT_DEADLINE;
    loop {
        match TcpClient::connect_endpoint(&endpoint, &parameters) {
            Err(NetworkError::ConnectFailed) if Instant::now() < deadline => {
                eprintln!("airshell-connect: {}", NetworkError::ConnectFailed);
                thread::sleep(Duration::from_secs(1));
            }
            result => return result,
        }
    }
}

/// Send stdin to the connection; at EOF send a final message (TCP half-close).
fn forward_stdin(conn: &TcpClient) {
    let mut stdin = std::io::stdin().lock();
    let mut buf = vec![0u8; CHUNK];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if conn.send(&buf[..n]).is_err() {
                    return;
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    if let Ok(mut fin) = ContentContext::new("stdin-eof") {
        fin.set_is_final(true);
        let _ = conn.send_with_context(&[], &fin);
    }
}
