//! Listens on AWDL (and other interfaces) and relays each connection to the local sshd.

use std::process::exit;
use std::thread;

use airshell::relay::relay_to;
use airshell::{SERVICE_TYPE, peer_to_peer_tcp};
use networkframework::{AdvertiseDescriptor, NetworkError, TcpListener};

fn main() {
    let listener = match listen() {
        Ok(listener) => listener,
        Err(error) => {
            println!("listener: failed({error})");
            exit(1);
        }
    };
    println!("listener: ready");

    loop {
        match listener.accept() {
            Ok(inbound) => {
                thread::spawn(move || {
                    if let Err(error) = relay_to(inbound, "127.0.0.1", 22) {
                        eprintln!("airshell-sshd: {error}");
                    }
                });
            }
            Err(error) => {
                println!("listener: failed({error})");
                exit(1);
            }
        }
    }
}

/// A peer-to-peer TCP listener advertised as `_awdlssh._tcp` under the computer name.
fn listen() -> Result<TcpListener, NetworkError> {
    let parameters = peer_to_peer_tcp()?;
    let descriptor = AdvertiseDescriptor::bonjour_service(None, SERVICE_TYPE, None)?;
    TcpListener::builder(&parameters)
        .advertise(descriptor)
        .on_advertised_endpoint(|endpoint, added| {
            let Some(endpoint) = endpoint else { return };
            let name = endpoint.bonjour_service_name().unwrap_or_default();
            let service_type = endpoint.bonjour_service_type().unwrap_or_default();
            let domain = endpoint.bonjour_service_domain().unwrap_or_default();
            if added {
                println!("Broadcasting as: {name} [{service_type}{domain}]");
            } else {
                println!("Unregistered: {name}.{service_type}{domain}");
            }
        })
        .bind()
}
