//! Advertises a peer-to-peer (AWDL) service and relays each accepted connection
//! to a local TCP service (sshd by default, but any host:port).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use airshell::relay::{Transferred, relay_to};
use airshell::{SERVICE_TYPE, logging, peer_to_peer_tcp};
use clap::Parser;
use networkframework::{AdvertiseDescriptor, NetworkError, TcpListener};
use tracing::{error, info, info_span, warn};

/// Advertise a peer-to-peer (AWDL) service and relay each connection to a local TCP service.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Port of the local service to relay accepted connections to (sshd).
    #[arg(short, long, env = "AIRSHELL_PORT", default_value_t = 22)]
    port: u16,

    /// Host of the local service to relay to.
    #[arg(long, env = "AIRSHELL_HOST", default_value = "127.0.0.1")]
    host: String,

    /// Bonjour service name to advertise (defaults to the computer name).
    #[arg(short, long, env = "AIRSHELL_NAME")]
    name: Option<String>,

    /// Bonjour service type to advertise.
    #[arg(long, env = "AIRSHELL_SERVICE_TYPE", default_value = SERVICE_TYPE)]
    service_type: String,

    /// Append logs to this file in addition to stderr. Defaults to a per-instance
    /// path under ~/Library/Logs so multiple proxies don't share one log.
    #[arg(long, env = "AIRSHELL_LOG_FILE", value_name = "PATH")]
    log_file: Option<PathBuf>,
}

/// Per-instance default log path: keyed on the advertised name, else the port
/// (the name defaults to the computer name inside Network.framework, which this
/// process never sees). Lands in ~/Library/Logs, falling back to the cwd.
fn default_log_path(cli: &Cli) -> PathBuf {
    let slug = cli
        .name
        .as_deref()
        .map(slugify)
        .unwrap_or_else(|| format!("port-{}", cli.port));
    let file = format!("airshell-proxy-{slug}.log");
    match std::env::var_os("HOME") {
        Some(home) => PathBuf::from(home).join("Library/Logs").join(file),
        None => PathBuf::from(file),
    }
}

/// Make a string safe for a filename: keep alphanumerics, collapse the rest to '-'.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let log_file = cli.log_file.clone().unwrap_or_else(|| default_log_path(&cli));
    let _guard = match logging::init(Some(&log_file)) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("airshell-proxy: cannot open log file {}: {error}", log_file.display());
            return ExitCode::FAILURE;
        }
    };

    let listener = match listen(cli.name.as_deref(), &cli.service_type) {
        Ok(listener) => listener,
        Err(error) => {
            error!(%error, "listener failed to bind");
            return ExitCode::FAILURE;
        }
    };
    info!(host = %cli.host, port = cli.port, "listener ready");

    // The accept loop blocks, so run it on its own thread and let main wait for
    // either Ctrl-C or a fatal accept error, then return so the log guard flushes.
    let (tx, rx) = mpsc::channel::<ExitCode>();
    if let Err(error) = ctrlc::set_handler({
        let tx = tx.clone();
        move || {
            let _ = tx.send(ExitCode::SUCCESS);
        }
    }) {
        warn!(%error, "no signal handler; Ctrl-C will not shut down cleanly");
    }

    let host: Arc<str> = Arc::from(cli.host);
    let port = cli.port;
    thread::spawn(move || tx.send(accept_loop(&listener, &host, port)));

    let code = rx.recv().unwrap_or(ExitCode::FAILURE);
    info!("shutting down");
    code
}

/// Accept connections forever, relaying each to `host:port` on its own thread.
/// Returns only on a fatal listener error.
fn accept_loop(listener: &TcpListener, host: &Arc<str>, port: u16) -> ExitCode {
    let counter = AtomicU64::new(0);
    loop {
        match listener.accept() {
            Ok(inbound) => {
                let id = counter.fetch_add(1, Ordering::Relaxed);
                let host = Arc::clone(host);
                thread::spawn(move || {
                    let span = info_span!("conn", id);
                    let _enter = span.enter();
                    info!("accepted");
                    match relay_to(inbound, &host, port) {
                        Ok(Transferred {
                            to_service,
                            to_peer,
                        }) => info!(to_service, to_peer, "closed"),
                        Err(error) => error!(%error, "relay failed"),
                    }
                });
            }
            Err(error) => {
                error!(%error, "accept failed");
                return ExitCode::FAILURE;
            }
        }
    }
}

/// A peer-to-peer TCP listener advertised as `service_type` under `name`
/// (or the computer name when `name` is `None`).
fn listen(name: Option<&str>, service_type: &str) -> Result<TcpListener, NetworkError> {
    let parameters = peer_to_peer_tcp()?;
    let descriptor = AdvertiseDescriptor::bonjour_service(name, service_type, None)?;
    TcpListener::builder(&parameters)
        .advertise(descriptor)
        .on_advertised_endpoint(|endpoint, added| {
            let Some(endpoint) = endpoint else { return };
            let name = endpoint.bonjour_service_name().unwrap_or_default();
            let service_type = endpoint.bonjour_service_type().unwrap_or_default();
            let domain = endpoint.bonjour_service_domain().unwrap_or_default();
            if added {
                info!(%name, service = %service_type, %domain, "broadcasting");
            } else {
                warn!(%name, service = %service_type, %domain, "unregistered");
            }
        })
        .bind()
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
    fn port_flag_parses() {
        let cli = Cli::try_parse_from(["airshell-proxy", "--port", "2222"]).unwrap();
        assert_eq!(cli.port, 2222);
    }

    #[test]
    fn slugify_keeps_alphanumerics_and_collapses_the_rest() {
        assert_eq!(super::slugify("Pedro's MacBook Pro"), "Pedro-s-MacBook-Pro");
        assert_eq!(super::slugify("db_01"), "db-01");
    }

    #[test]
    fn default_log_path_falls_back_to_port() {
        let cli = Cli::try_parse_from(["airshell-proxy", "--port", "5432"]).unwrap();
        let path = super::default_log_path(&cli);
        assert!(
            path.ends_with("airshell-proxy-port-5432.log"),
            "got {}",
            path.display()
        );
    }
}
