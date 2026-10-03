# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

airshell bridges nearby devices over peer-to-peer Wi-Fi radios (AWDL today, Wi-Fi Aware in
progress) into plain TCP byte streams, so existing tools work unchanged. The shipping use case is
SSH between two Macs over AWDL with no network in between. See `README.md` for the user-facing
story and the full quick-start.

Two halves:
- **Transport** — the radio link, exposed through `networkframework` (Network.framework bindings).
- **Application** — the relay binaries that sit on top. Currently just the SSH/TCP relay.

## Commands

```bash
cargo build                       # debug build of both binaries
cargo build --release             # target/release/airshell-proxy, airshell-connect
cargo test                        # unit tests only; network tests are #[ignore]d (see below)
cargo test slugify                # run a single test by name substring

scripts/remote-test.sh <host>     # build here, run ALL tests (incl. ignored) on a test Mac over SSH
scripts/deploy.sh <host> [host…]  # release build + install into ~/.local/bin on each host
scripts/build-mac.sh              # Nix build of both macOS apps for macOS 26+ (needs Xcode)
scripts/build-ios.sh              # static lib for iOS, with the wifi-aware feature (run in nix develop)
```

**Run cargo inside `nix develop`** (auto-loaded via direnv). The dev shell is NoCC, so the host
Xcode clang + macOS SDK are used directly and the Swift bridge in `networkframework`'s `build.rs`
links cleanly — `cargo build`/`test` and `scripts/remote-test.sh` all just work there. It also
provides the Rust toolchain with the iOS target and `jq`.

Outside the shell, a Nix cc wrapper shadows Xcode's clang and targets the Nix SDK, so the final
link fails (missing Swift runtime / wrong min-OS). `cargo check` / `cargo clippy` don't link, so
they work anywhere.

## Testing model — important

Any test that touches Network.framework / AWDL is marked `#[ignore]` because it needs real radio
hardware and can't run in CI or on a single machine. `cargo test` therefore only covers CLI
parsing and pure helpers. **To actually exercise the relay you need two Macs**: run
`scripts/remote-test.sh <host>`, which cross-builds locally and runs the ignored tests on the
remote Mac with `--include-ignored`. Integration tests find the binaries via the
`AIRSHELL_BIN_DIR` env var (set by that script); `tests/common` provides echo/banner servers and a
`relay_listener` helper.

## Architecture

Request flow (`README.md` has the diagram):

```
ssh → airshell-connect ══ AWDL ══▶ airshell-proxy → local sshd
      (resolves peer by Bonjour name)  (advertises the service)
```

- `src/lib.rs` — the shared seam: `SERVICE_TYPE` (`_awdlssh._tcp`) and `peer_to_peer_tcp()`, which
  is the one thing that opts a connection into AWDL (`set_include_peer_to_peer(true)`). Ordinary
  sockets never bring the radio up.
- `src/relay.rs` — `pipe()` copies one direction and cancels **both** connections on EOF (graceful
  FIN, which also unblocks the opposite receive); `relay_to()` runs the two directions on two
  threads and reports byte counts.
- `src/bin/airshell-proxy.rs` — daemon. Advertises one service (from flags) or many (from
  `src/config.rs`, loaded from `--config` or `~/.config/airshell/config.toml` when present), one
  listener thread each. `main` waits on a channel for Ctrl-C or for every listener to die, then
  returns cleanly to flush the log guard. File log format is `--log-format` (auto = text for one
  service, JSON for several).
- `src/config.rs` — `config.toml` parsing/validation: `[[service]]` list, defaults, and the
  `(name, service_type)` uniqueness check. `name` is required per service (the flag path can omit
  it; config can't, or unnamed services collide).
- `src/bin/airshell-connect.rs` — ssh `ProxyCommand`. Resolves a peer by name, bridges
  stdin/stdout. **stdout is the ssh data channel, so it must never carry logs** — logging goes to
  stderr/file only. Retries `connect_endpoint` for `--timeout` seconds while the peer is being
  discovered.
- `src/logging.rs`, `src/error.rs` — shared `tracing` setup and the relay error type.

Every CLI flag has an `AIRSHELL_*` env var (clap `env` feature) for launchd/systemd units.

## Conventions and gotchas

- **Edition 2024**, `panic = "abort"` in both profiles.
- `networkframework` comes from a **fork** (`github.com/pplanel/networkframework-rs`, branch
  `airshell`) pinned in `Cargo.toml` until changes land upstream. The Nix build pins its source
  hash in `flake.nix` (`outputHashes`) — bumping the dep means updating that hash too.
- The Nix macOS build is deliberately **impure** (`__noChroot`): it unsets the Nix SDK and uses
  Xcode's clang/swift/xcrun + the macOS 26 SDK, because `networkframework`'s `build.rs` compiles a
  Swift bridge. Xcode must be installed.
- **Deploying binaries:** never overwrite a running binary in place — macOS caches the old code
  signature on the inode and SIGKILLs the replacement ("killed", no message). Copy to `*.new` and
  `mv` into place (`scripts/deploy.sh` does this).
- **macOS firewall / Local Network privacy** can silently block the proxy on first run; see the
  "macOS notes" section of `README.md`.
- `swift/AirshellWiFiAware/` holds the in-progress Wi-Fi Aware work (iOS 26+); the committed tree
  is mostly SwiftPM build artifacts.

## Security

airshell does not authenticate or encrypt streams itself — anyone in radio range can reach an
advertised service. Only expose services that authenticate themselves (SSH), or use a transport
that pairs devices first (Wi-Fi Aware).
