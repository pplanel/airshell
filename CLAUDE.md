# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

macOS-only Rust project: SSH between Macs over AWDL (Apple's peer-to-peer Wi-Fi) using Network.framework via the `networkframework` crate. Two binaries:

- `airshell-sshd` — Network.framework TCP listener with peer-to-peer enabled, advertised as Bonjour `_awdlssh._tcp` under the computer name; relays each accepted connection to `127.0.0.1:22`.
- `airshell-connect <ServiceName>` — ssh `ProxyCommand`; resolves `<ServiceName>._awdlssh._tcp.local.` over peer-to-peer and bridges stdin/stdout.

## Commands

```bash
cargo build --release                     # build both binaries
scripts/remote-test.sh <host>             # build tests locally, run them on <host> over ssh
scripts/deploy.sh <host> [host...]        # build release, install to ~/.local/bin on each host
cargo clippy --all-targets --features wifi-aware   # type-check the iOS-only Wi-Fi Aware module on macOS
nix develop                               # shell with the aarch64-apple-ios Rust target, using Xcode's SDKs
scripts/build-ios.sh                      # (in nix develop) libairshell.a for the iOS app, with wifi-aware
(cd swift/AirshellWiFiAware && xcodebuild -scheme AirshellWiFiAware -destination 'generic/platform=iOS' build)
```

Plain `cargo test` runs only the in-memory relay unit test in `src/relay.rs`. Every test that opens sockets or advertises over Bonjour (including the one in `src/lib.rs`) is `#[ignore]`d. `remote-test.sh` copies the test executables plus the debug binaries to `/tmp/airshell-test` on the host and runs each with `--include-ignored --test-threads=1` and `AIRSHELL_BIN_DIR` set (see `tests/common/mod.rs::bin`). Requires `jq` locally.

## Architecture notes

- `src/lib.rs::peer_to_peer_tcp()` is the single place that builds connection parameters (plain TCP + `set_include_peer_to_peer(true)`). Both binaries and the tests must use it; standard sockets do not trigger AWDL.
- `src/transport.rs`: the `Transport` trait (`receive`/`send`/`cancel`) is all the relay needs from a connection; `TcpClient` implements it. New backends (e.g. a Wi-Fi Aware Swift shim) implement this trait; listening/connecting stays backend-specific.
- Wi-Fi Aware (iOS/iPadOS only; unavailable on macOS) lives behind the `wifi-aware` feature: `src/wifi_aware.rs` declares the `airshell_wa_*` C functions that `swift/AirshellWiFiAware` exports with `@_cdecl` (Exports.swift is the ABI; keep both sides in sync). The symbols resolve only when an iOS app links both, so on macOS this module can be checked but not linked or run. See that package's README for app requirements and what is unverified on devices.
- `src/relay.rs`: `relay(a, b)` runs one `pipe` direction on a scoped helper thread and the other on the caller's thread; `relay_to` connects outbound to sshd and calls it. When either direction ends, `pipe` calls `cancel()` on **both** connections — this is what unblocks the opposite `receive` and produces a graceful FIN. Teardown correctness depends on this; the tests check for no hangs on EOF / unreachable sshd.
- `airshell-connect` signals stdin EOF by sending an empty message with a `ContentContext` marked final (TCP half-close), and retries `ConnectFailed` for up to 30s because the crate fails on the first `waiting` state.
- Blocking threads, no async runtime. `panic = "abort"` in both profiles.
- `networkframework` is a git dependency on the `pplanel/networkframework-rs` `airshell` branch (fork adding listener advertise, `connect_endpoint`, graceful cancel). API changes there may need to be made in the fork.
- Integration tests reproduce the daemon in-process via `tests/common::relay_listener` (with echo/banner/closed-port stand-ins for sshd) and drive the real `airshell-connect` binary as a subprocess. Test Bonjour names include the PID to avoid collisions.

## macOS operational gotchas

- Replacing a binary in place causes the kernel to SIGKILL it on launch (stale code-signature cache on the inode). Always copy to `*.new` and `mv -f` into place, as `deploy.sh` does.
- First listen triggers the Application Firewall prompt; unanswered, connections sit in `waiting`. Pre-approve with `socketfilterfw --add/--unblockapp`.
- Local Network privacy: if `airshell-sshd` hangs at `listener: waiting(...)` when started over SSH, run it once from a local Terminal and accept the prompt.
- `airshell-sshd` prints `listener: ready` / `Broadcasting as: ...` to stdout; the README documents these strings.
