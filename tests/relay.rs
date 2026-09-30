mod common;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};

use common::{TIMEOUT, echo_server, relay_listener};

#[test]
#[ignore = "network: run on caladan/imperium via scripts/remote-test.sh"]
fn relays_one_mebibyte_both_ways_and_tears_down_on_close() {
    let port = relay_listener(echo_server(), None);

    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.set_read_timeout(Some(TIMEOUT)).unwrap();
    let payload: Vec<u8> = (0..1024 * 1024u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
        .collect();

    let mut writer = client.try_clone().unwrap();
    let to_send = payload.clone();
    let sender = std::thread::spawn(move || writer.write_all(&to_send).unwrap());

    let mut echoed = vec![0u8; payload.len()];
    client.read_exact(&mut echoed).unwrap();
    sender.join().unwrap();
    assert!(echoed == payload, "echoed bytes differ from what was sent");

    // Closing our write side must make the relay cancel both connections,
    // which we observe as EOF on our read side.
    client.shutdown(Shutdown::Write).unwrap();
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty(), "unexpected {} trailing bytes", rest.len());
}

#[test]
#[ignore = "network: run on caladan/imperium via scripts/remote-test.sh"]
fn immediate_client_eof_closes_without_hanging() {
    let port = relay_listener(echo_server(), None);
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client.set_read_timeout(Some(TIMEOUT)).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut rest = Vec::new();
    client.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
}
