mod common;

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{banner_server, bin, closed_port, echo_server, relay_listener};

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> std::process::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("airshell-connect did not exit within {timeout:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "network: run on a test Mac via scripts/remote-test.sh <host>"]
fn round_trips_through_bonjour_and_exits_zero_on_stdin_eof() {
    let name = format!("airshell test's {}", std::process::id());
    relay_listener(echo_server(), Some(&name));

    let mut child = Command::new(bin("airshell-connect"))
        .arg(&name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let message = b"hello airshell\n";
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(message).unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut echoed = vec![0u8; message.len()];
    stdout.read_exact(&mut echoed).unwrap();
    assert_eq!(echoed, message);

    drop(stdin); // EOF -> final message -> relay tears down -> airshell-connect exits
    let status = wait_with_timeout(&mut child, Duration::from_secs(10));
    assert_eq!(status.code(), Some(0));
}

#[test]
#[ignore = "runs an airshell binary: run on a test Mac via scripts/remote-test.sh <host>"]
fn missing_argument_prints_usage_and_exits_two() {
    let output = Command::new(bin("airshell-connect")).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "usage: airshell-connect <ServiceName>\n"
    );
}

#[test]
#[ignore = "network: run on a test Mac via scripts/remote-test.sh <host>"]
fn server_banner_arrives_before_client_sends_anything_twenty_times() {
    const BANNER: &[u8] = b"SSH-2.0-airshell-test\r\n";
    let name = format!("airshell-banner-{}", std::process::id());
    relay_listener(banner_server(BANNER), Some(&name));

    for run in 1..=20 {
        let mut child = Command::new(bin("airshell-connect"))
            .arg(&name)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap(); // held open: we send nothing
        let mut stdout = child.stdout.take().unwrap();
        let mut banner = vec![0u8; BANNER.len()];
        stdout
            .read_exact(&mut banner)
            .unwrap_or_else(|e| panic!("run {run}: no banner: {e}"));
        assert_eq!(banner, BANNER, "run {run}");
        drop(stdin);
        let status = wait_with_timeout(&mut child, Duration::from_secs(10));
        assert_eq!(status.code(), Some(0), "run {run}");
    }
}

#[test]
#[ignore = "network: run on a test Mac via scripts/remote-test.sh <host>"]
fn unreachable_sshd_makes_airshell_connect_exit_instead_of_hanging() {
    let name = format!("airshell-refused-{}", std::process::id());
    relay_listener(closed_port(), Some(&name));

    let mut child = Command::new(bin("airshell-connect"))
        .arg(&name)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let _stdin = child.stdin.take().unwrap(); // held open: exit must come from the relay closing
    wait_with_timeout(&mut child, Duration::from_secs(10));
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    assert!(out.is_empty(), "unexpected output: {out:?}");
}
