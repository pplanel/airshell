//! Shared helpers for the networked integration tests.
//!
//! These tests open sockets and advertise over Bonjour, so they are
//! `#[ignore]`d: build them on Arrakis, run them on caladan / imperium via
//! `scripts/remote-test.sh`.

#![allow(dead_code)]

use std::io::Write;
use std::net::{Shutdown, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use airshell::relay::relay_to;
use airshell::{SERVICE_TYPE, peer_to_peer_tcp};
use networkframework::{AdvertiseDescriptor, TcpListener as NwListener};

pub const TIMEOUT: Duration = Duration::from_secs(10);

/// A loopback TCP echo server standing in for sshd. Returns its port.
pub fn echo_server() -> u16 {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in server.incoming() {
            let mut writer = stream.unwrap();
            thread::spawn(move || {
                let mut reader = writer.try_clone().unwrap();
                let _ = std::io::copy(&mut reader, &mut writer);
                let _ = writer.shutdown(Shutdown::Write);
            });
        }
    });
    port
}

/// A server that speaks first, like sshd: sends `banner` on accept, then
/// holds the connection until the client closes. Returns its port.
pub fn banner_server(banner: &'static [u8]) -> u16 {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in server.incoming() {
            let mut stream = stream.unwrap();
            thread::spawn(move || {
                let _ = stream.write_all(banner);
                let _ = std::io::copy(&mut stream.try_clone().unwrap(), &mut std::io::sink());
            });
        }
    });
    port
}

/// A loopback port with nothing listening on it (sshd switched off).
pub fn closed_port() -> u16 {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    server.local_addr().unwrap().port()
}

/// What `airshell-sshd` does, but relaying to `127.0.0.1:target_port` and
/// optionally advertising under `advertise_name`. The listener lives on a
/// background thread for the rest of the test process. Returns its port once
/// it is ready.
pub fn relay_listener(target_port: u16, advertise_name: Option<&str>) -> u16 {
    let advertise_name = advertise_name.map(str::to_owned);
    let (port_tx, port_rx) = mpsc::channel();
    thread::spawn(move || {
        let parameters = peer_to_peer_tcp().unwrap();
        let mut builder = NwListener::builder(&parameters);
        if let Some(name) = &advertise_name {
            builder = builder.advertise(
                AdvertiseDescriptor::bonjour_service(Some(name), SERVICE_TYPE, None).unwrap(),
            );
        }
        let listener = builder.bind().unwrap();
        port_tx.send(listener.local_port()).unwrap();
        while let Ok(inbound) = listener.accept() {
            thread::spawn(move || {
                let _ = relay_to(inbound, "127.0.0.1", target_port);
            });
        }
    });
    port_rx
        .recv_timeout(TIMEOUT)
        .expect("listener never became ready")
}

/// Path of an airshell binary: `$AIRSHELL_BIN_DIR/<name>` when set (remote
/// runs), else the one Cargo built for this test.
pub fn bin(name: &str) -> PathBuf {
    match std::env::var_os("AIRSHELL_BIN_DIR") {
        Some(dir) => PathBuf::from(dir).join(name),
        None => match name {
            "airshell-connect" => PathBuf::from(env!("CARGO_BIN_EXE_airshell-connect")),
            "airshell-sshd" => PathBuf::from(env!("CARGO_BIN_EXE_airshell-sshd")),
            other => panic!("unknown binary {other}"),
        },
    }
}
