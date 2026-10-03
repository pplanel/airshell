//! ssh ProxyCommand: connects to a peer's airshell-proxy by Bonjour name and bridges stdin/stdout.

use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use airshell::logging::{self, Format};
use airshell::relay::CHUNK;
use airshell::{SERVICE_TYPE, peer_to_peer_tcp};
use clap::Parser;
use networkframework::{ContentContext, Endpoint, NetworkError, TcpClient};
use tracing::{error, info, warn};

/// Connect to a peer's airshell-proxy over AWDL and bridge stdin/stdout (an ssh ProxyCommand).
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Bonjour service name of the peer's airshell-proxy.
    service_name: String,

    /// Give up connecting after this many seconds.
    #[arg(short, long, env = "AIRSHELL_TIMEOUT", default_value_t = 30)]
    timeout: u64,

    /// Bonjour service type to resolve.
    #[arg(long, env = "AIRSHELL_SERVICE_TYPE", default_value = SERVICE_TYPE)]
    service_type: String,

    /// Bonjour domain to resolve the service in.
    #[arg(long, env = "AIRSHELL_DOMAIN", default_value = "local.")]
    domain: String,

    /// Append logs to this file (stdout is the data channel, so logs never go there).
    #[arg(long, env = "AIRSHELL_LOG_FILE", value_name = "PATH")]
    log_file: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let _guard = match logging::init(cli.log_file.as_deref(), Format::Text) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("airshell-connect: cannot open log file: {error}");
            return ExitCode::FAILURE;
        }
    };

    let deadline = Duration::from_secs(cli.timeout);
    let conn = match connect(&cli.service_name, &cli.service_type, &cli.domain, deadline) {
        Ok(conn) => Arc::new(conn),
        Err(error) => {
            error!(service = %cli.service_name, %error, "connect failed");
            return ExitCode::FAILURE;
        }
    };
    info!(service = %cli.service_name, "connected");

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
    info!("disconnected");
    ExitCode::SUCCESS
}

/// Connect to `<service_name>.<service_type>.<domain>` with peer-to-peer enabled.
/// The crate fails a connect on its first `waiting` error, so report each
/// failure and retry until the deadline elapses.
fn connect(
    service_name: &str,
    service_type: &str,
    domain: &str,
    timeout: Duration,
) -> Result<TcpClient, NetworkError> {
    let parameters = peer_to_peer_tcp()?;
    let endpoint = Endpoint::bonjour_service(Some(service_name), service_type, Some(domain))?;
    let deadline = Instant::now() + timeout;
    loop {
        match TcpClient::connect_endpoint(&endpoint, &parameters) {
            Err(NetworkError::ConnectFailed) if Instant::now() < deadline => {
                warn!(error = %NetworkError::ConnectFailed, "retrying");
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn service_name_is_required() {
        assert!(Cli::try_parse_from(["airshell-connect"]).is_err());
    }

    #[test]
    fn parses_service_name_and_timeout() {
        let cli = Cli::try_parse_from(["airshell-connect", "Target-Mac", "--timeout", "5"]).unwrap();
        assert_eq!(cli.service_name, "Target-Mac");
        assert_eq!(cli.timeout, 5);
    }
}
