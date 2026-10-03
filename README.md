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
| Application | **SSH**: `airshell-proxy` and `airshell-connect` | Working |
| Application | Relaying any local TCP service, not only sshd (`--port`/`--host`) | Working |

## How it works

```
 device A                                                       device B
 application ── airshell client ══ peer-to-peer link ══▶ airshell daemon ── local service
 (ssh)          (finds B by name)   (AWDL, Wi-Fi Aware)  (advertises)        (sshd)
```

- **Discovery:** the daemon advertises a service over the peer-to-peer link. On AWDL that's a
  Bonjour `_awdlssh._tcp` record under the Mac's computer name; the client looks it up by name.
  Both sides accept `--service-type` to advertise/resolve a different record.
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
cargo build --release              # target/release/airshell-proxy, airshell-connect
scripts/deploy.sh mac-a mac-b      # or: build, then install into ~/.local/bin on each host over SSH
```

**2. On the Mac you connect to,** turn on Remote Login (System Settings > General > Sharing) so
sshd listens on `127.0.0.1:22`, then start the daemon:

```bash
~/.local/bin/airshell-proxy
```

```text
INFO airshell_proxy: listener ready host=127.0.0.1 port=22
INFO airshell_proxy: broadcasting name=Target-MacBook service=_awdlssh._tcp domain=local.
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

## Relaying other services

`airshell-proxy` relays to `127.0.0.1:22` by default, but any local TCP service works:

```bash
airshell-proxy --port 5432 --name db-mac           # expose Postgres over AWDL
airshell-connect db-mac | …                         # the client side is service-agnostic
```

Remember that AWDL has no authentication — expose only services that authenticate
themselves (like SSH), or pair devices with a transport that does.

## Configuration and logging

Every flag has an `AIRSHELL_*` environment variable (handy for launchd/systemd units):
`--port`→`AIRSHELL_PORT`, `--host`, `--name`, `--service-type`, `--log-file`; and on the
client `--timeout`, `--domain`. Run either binary with `--help` for the full list.

Both log via `tracing`; the level comes from `RUST_LOG` (default `info`). `--log-file PATH`
appends logs there in addition to stderr. `airshell-connect` never logs to stdout — that stays
the ssh data channel. `airshell-proxy` shuts down cleanly on Ctrl-C (flushing its log).

## macOS notes

- **Application Firewall:** the first time `airshell-proxy` listens, macOS asks whether to allow it.
  Until someone answers, incoming connections hang. On headless Macs, pre-approve it:
  ```bash
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add ~/.local/bin/airshell-proxy
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp ~/.local/bin/airshell-proxy
  ```
- **Local Network privacy:** if `airshell-proxy` never logs `listener ready` when started over
  SSH, run it once from a local Terminal and accept the prompt.
- **Run it as a service:** `packaging/sh.airshell.proxy.plist` is a LaunchAgent template
  (edit the `USERNAME` placeholders) that keeps `airshell-proxy` running and logging.
- **Replacing binaries:** overwriting a binary in place makes macOS kill the new one on launch
  (a stale code-signature cache). Copy to a temporary name and rename it into place;
  `scripts/deploy.sh` does this.

## Development

```bash
cargo build
cargo test                        # tests that need the network are #[ignore]d
scripts/build-mac.sh              # build both macOS apps for macOS 26+ via Nix (needs Xcode)
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
| `src/relay.rs` | Two-way byte relay with graceful teardown and byte counts |
| `src/error.rs` | Relay error type (`thiserror`) |
| `src/logging.rs` | Shared `tracing` setup (stderr + optional file) |
| `src/bin/airshell-proxy.rs` | Daemon: advertises over AWDL, relays to a local TCP service |
| `src/bin/airshell-connect.rs` | `ProxyCommand`: finds a peer by name, bridges stdin/stdout |
| `tests/` | Relay and `airshell-connect` integration tests |
| `scripts/` | Build, deploy to hosts, run tests on a remote Mac |
| `packaging/` | launchd LaunchAgent template for `airshell-proxy` |
