//! Advertises one or more peer-to-peer (AWDL) services and relays each accepted
//! connection to a local TCP service (sshd by default, but any host:port).
//!
//! Runs a single service from flags, or several from a config file
//! (`--config`, else `~/.config/airshell/config.toml` when present). With more
//! than one service the shared log defaults to JSON; see `--log-format`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread;

use airshell::config::{self, Config};
use airshell::logging::{self, Format};
use airshell::relay::{Transferred, relay_to};
use airshell::{SERVICE_TYPE, peer_to_peer_tcp};
use clap::{Parser, ValueEnum};
use networkframework::{AdvertiseDescriptor, NetworkError, TcpListener};
use tracing::{error, info, info_span, warn};

/// Advertise peer-to-peer (AWDL) services and relay each connection to a local TCP service.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Config file defining multiple [[service]] entries. When omitted, a config
    /// at ~/.config/airshell/config.toml is used if present, else the flags below.
    #[arg(short, long, env = "AIRSHELL_CONFIG", value_name = "PATH")]
    config: Option<PathBuf>,

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

    /// Log file format. `auto` is text for a single service, JSON for several.
    #[arg(long, env = "AIRSHELL_LOG_FORMAT", value_enum, default_value_t = LogFormat::Auto)]
    log_format: LogFormat,

    /// Append logs to this file in addition to stderr. Defaults to a per-instance
    /// path under ~/Library/Logs so multiple proxies don't share one log.
    #[arg(long, env = "AIRSHELL_LOG_FILE", value_name = "PATH")]
    log_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum LogFormat {
    Auto,
    Text,
    Json,
}

/// One service to advertise and relay: a resolved row from either the config or the flags.
struct ServiceSpec {
    name: Option<String>,
    service_type: String,
    host: String,
    port: u16,
}

impl ServiceSpec {
    /// Stable identifier for the log `service` field and the default log path:
    /// the advertised name, else `port-<port>` (the name defaults to the computer
    /// name inside Network.framework, which this process never sees).
    fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("port-{}", self.port))
    }
}

/// Why a listener died, so `main` can tell Ctrl-C from an accept failure.
enum Event {
    /// Ctrl-C: shut the whole proxy down now.
    Shutdown,
    /// One listener's accept loop returned on a fatal error.
    ListenerDied,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let services = match resolve_services(&cli) {
        Ok(services) => services,
        Err(error) => {
            eprintln!("airshell-proxy: {error}");
            return ExitCode::FAILURE;
        }
    };

    let format = match cli.log_format {
        LogFormat::Text => Format::Text,
        LogFormat::Json => Format::Json,
        LogFormat::Auto if services.len() > 1 => Format::Json,
        LogFormat::Auto => Format::Text,
    };
    let log_file = cli.log_file.clone().unwrap_or_else(|| default_log_path(&services));
    let _guard = match logging::init(Some(&log_file), format) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("airshell-proxy: cannot open log file {}: {error}", log_file.display());
            return ExitCode::FAILURE;
        }
    };

    // Bind every service up front; log and skip ones that fail. A service with no
    // live listener is pointless, so only a total bind failure is fatal.
    let (tx, rx) = mpsc::channel::<Event>();
    let mut alive = 0usize;
    for spec in services {
        let listener = match listen(spec.name.as_deref(), &spec.service_type) {
            Ok(listener) => listener,
            Err(error) => {
                error!(service = %spec.label(), %error, "listener failed to bind");
                continue;
            }
        };
        info!(service = %spec.label(), host = %spec.host, port = spec.port, "listener ready");
        alive += 1;
        let tx = tx.clone();
        thread::spawn(move || {
            accept_loop(&listener, &spec);
            let _ = tx.send(Event::ListenerDied);
        });
    }
    if alive == 0 {
        error!("no services could bind");
        return ExitCode::FAILURE;
    }

    if let Err(error) = ctrlc::set_handler({
        let tx = tx.clone();
        move || {
            let _ = tx.send(Event::Shutdown);
        }
    }) {
        warn!(%error, "no signal handler; Ctrl-C will not shut down cleanly");
    }

    // Wait for Ctrl-C, or for every listener to die unexpectedly, then return so
    // the log guard flushes.
    let code = loop {
        match rx.recv() {
            Ok(Event::Shutdown) | Err(_) => break ExitCode::SUCCESS,
            Ok(Event::ListenerDied) => {
                alive -= 1;
                if alive == 0 {
                    error!("all listeners stopped");
                    break ExitCode::FAILURE;
                }
            }
        }
    };
    info!("shutting down");
    code
}

/// Resolve the services to run: an explicit `--config` (error if unreadable),
/// else a config at the default path when present, else a single service from
/// the flags (preserving the original single-service behaviour).
fn resolve_services(cli: &Cli) -> Result<Vec<ServiceSpec>, config::ConfigError> {
    let path = cli.config.clone().or_else(|| {
        // An implicit default config is used only if it actually exists.
        config::default_path().filter(|p| p.is_file())
    });
    match path {
        Some(path) => Ok(Config::load(&path)?.service.into_iter().map(Into::into).collect()),
        None => Ok(vec![ServiceSpec {
            name: cli.name.clone(),
            service_type: cli.service_type.clone(),
            host: cli.host.clone(),
            port: cli.port,
        }]),
    }
}

impl From<config::Service> for ServiceSpec {
    fn from(s: config::Service) -> Self {
        ServiceSpec {
            name: Some(s.name),
            service_type: s.service_type,
            host: s.host,
            port: s.port,
        }
    }
}

/// Default log path. A single service keeps a per-instance name keyed on its
/// label; several services share one `airshell-proxy.log`. Lands in
/// ~/Library/Logs, falling back to the cwd.
fn default_log_path(services: &[ServiceSpec]) -> PathBuf {
    let file = match services {
        [only] => format!("airshell-proxy-{}.log", slugify(&only.label())),
        _ => "airshell-proxy.log".to_owned(),
    };
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

/// Accept connections forever, relaying each to the service's `host:port` on its
/// own thread. Returns only on a fatal listener error.
fn accept_loop(listener: &TcpListener, spec: &ServiceSpec) {
    let label = spec.label();
    let host: Arc<str> = Arc::from(spec.host.as_str());
    let counter = AtomicU64::new(0);
    loop {
        match listener.accept() {
            Ok(inbound) => {
                let id = counter.fetch_add(1, Ordering::Relaxed);
                let host = Arc::clone(&host);
                let label = label.clone();
                let port = spec.port;
                thread::spawn(move || {
                    let span = info_span!("conn", service = %label, id);
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
                error!(service = %label, %error, "accept failed");
                return;
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
    use super::{Cli, ServiceSpec};
    use clap::{CommandFactory, Parser};

    fn spec(name: Option<&str>, port: u16) -> ServiceSpec {
        ServiceSpec {
            name: name.map(str::to_owned),
            service_type: "_awdlssh._tcp".to_owned(),
            host: "127.0.0.1".to_owned(),
            port,
        }
    }

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
    fn single_service_log_path_keys_on_label() {
        let path = super::default_log_path(&[spec(None, 5432)]);
        assert!(
            path.ends_with("airshell-proxy-port-5432.log"),
            "got {}",
            path.display()
        );
        let path = super::default_log_path(&[spec(Some("db mac"), 5432)]);
        assert!(
            path.ends_with("airshell-proxy-db-mac.log"),
            "got {}",
            path.display()
        );
    }

    #[test]
    fn multi_service_log_path_is_shared() {
        let path = super::default_log_path(&[spec(Some("ssh"), 22), spec(Some("db"), 5432)]);
        assert!(path.ends_with("airshell-proxy.log"), "got {}", path.display());
    }
}
