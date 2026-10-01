# airshell

Connections between nearby devices with no network in between: no router, no internet, no cables.

Laptops and phones can talk directly over peer-to-peer Wi-Fi links built into their radios, but
outside a few system features (AirDrop, Sidecar) ordinary tools can't use them. airshell turns
those links into plain byte streams and relays them to services you already run, so existing
tools work unchanged. The radio link is a pluggable **transport**; **applications** sit on top.

SSH between Macs over AWDL is the first combination, and it works today.

## Status

| Kind | What | Status |
|------|------|--------|
| Transport | **AWDL** (Apple Wireless Direct Link) through Network.framework, Mac to Mac | Working |
| Transport | **Wi-Fi Aware** (the Wi-Fi Alliance standard, also called NAN) on iOS and iPadOS 26+ | In progress ([#1](https://github.com/pplanel/airshell/pull/1)) |
| Application | **SSH**: `airshell-sshd` and `airshell-connect` | Working |
| Application | Relaying any local TCP service, not only sshd | Planned |

## How it works

```
 device A                                                       device B
 application ── airshell client ══ peer-to-peer link ══▶ airshell daemon ── local service
 (ssh)          (finds B by name)   (AWDL, Wi-Fi Aware)  (advertises)        (sshd)
```

- **Discovery:** the daemon advertises a service over the peer-to-peer link. On AWDL that's a
  Bonjour `_awdlssh._tcp` record under the Mac's computer name; the client looks it up by name.
- **Link:** Network.framework brings up AWDL only for connections that allow peer-to-peer
  interfaces (`peer_to_peer_tcp()` in `src/lib.rs`); ordinary sockets never trigger it.
- **Relay:** the daemon connects each incoming stream to the local service and copies bytes in
  both directions on two threads. When either side finishes, both connections close with a FIN
  (`src/relay.rs`).

**Security:** airshell doesn't authenticate or encrypt streams itself, so anyone within radio range
can reach the advertised service. That's fine for SSH, which does both; other services need their
own authentication, or a transport that pairs devices first, such as Wi-Fi Aware.

## Quick start: SSH between two Macs

**1. Build and install** both binaries on both Macs:

```bash
cargo build --release              # target/release/airshell-sshd, airshell-connect
scripts/deploy.sh mac-a mac-b      # or: build, then install into ~/.local/bin on each host over SSH
```

**2. On the Mac you connect to,** turn on Remote Login (System Settings > General > Sharing) so
sshd listens on `127.0.0.1:22`, then start the daemon:

```bash
~/.local/bin/airshell-sshd
```

```text
listener: ready
Broadcasting as: Target-MacBook [_awdlssh._tcplocal.]
```

**3. On the Mac you connect from,** add a host to `~/.ssh/config` using the other Mac's name
(System Settings > General > About):

```ssh-config
Host target-mac
    User your-username
    ProxyCommand ~/.local/bin/airshell-connect "Target-MacBook"
```

**4. Go off-grid and connect.** Disconnect both Macs from Wi-Fi networks and cables (keep Wi-Fi
turned on), then:

```bash
ssh target-mac
```

`scp` and `rsync` work the same way. To see traffic on the AWDL interface: `netstat -I awdl0 -b 1`.

`airshell-connect` retries for up to 30 seconds while the peer is being discovered.

## macOS notes

- **Application Firewall:** the first time `airshell-sshd` listens, macOS asks whether to allow it.
  Until someone answers, incoming connections hang. On headless Macs, pre-approve it:
  ```bash
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add ~/.local/bin/airshell-sshd
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp ~/.local/bin/airshell-sshd
  ```
- **Local Network privacy:** if `airshell-sshd` never prints `listener: ready` when started over
  SSH, run it once from a local Terminal and accept the prompt.
- **Replacing binaries:** overwriting a binary in place makes macOS kill the new one on launch
  (a stale code-signature cache). Copy to a temporary name and rename it into place;
  `scripts/deploy.sh` does this.

## Development

```bash
cargo build
cargo test                        # tests that need the network are #[ignore]d
scripts/remote-test.sh <host>     # build here, run every test on a test Mac over SSH
scripts/deploy.sh <host> [host…]  # release build, installed into ~/.local/bin on each host
```

Network.framework bindings come from
[`networkframework`](https://github.com/doom-fish/networkframework-rs), currently through a fork
(see `Cargo.toml`).

## Repository layout

| Path | Purpose |
|------|---------|
| `src/lib.rs` | Bonjour service type and peer-to-peer connection parameters |
| `src/relay.rs` | Two-way byte relay with graceful teardown |
| `src/bin/airshell-sshd.rs` | Daemon: advertises over AWDL, relays to the local sshd |
| `src/bin/airshell-connect.rs` | SSH `ProxyCommand`: finds a peer by name, bridges stdin/stdout |
| `tests/` | Relay and `airshell-connect` integration tests |
| `scripts/` | Deploy to hosts, run tests on a remote Mac |
