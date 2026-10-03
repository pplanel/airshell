# SSH over AWDL with nix-darwin: a 3-Mac mesh

This tutorial wires airshell into a [nix-darwin](https://github.com/LnL7/nix-darwin) +
home-manager flake so three Apple-Silicon Macs — **arrakis**, **caladan**, and **imperium** — can
SSH to each other directly over peer-to-peer Wi-Fi (AWDL), with **no router, internet, or cable**
between them. Each host runs one `airshell-proxy` LaunchAgent advertising its sshd over AWDL, and
each host's `~/.ssh/config` gains a `ProxyCommand` entry for the other two.

It complements a tailnet rather than replacing it: Tailscale needs a network path, airshell needs
only radio range. When the three are off-grid in the same room, `ssh caladan` still works.

The snippets below follow the layout of `~/.config/nix-darwin` (a `flake.nix`, per-host files in
`hosts/darwin/`, feature modules in `modules/darwin/`). Adjust paths if yours differs.

## Prerequisites

- **Apple Silicon + macOS 26+** on every host. airshell's AWDL transport targets macOS 26.
- **Xcode installed on every host.** airshell's flake build compiles a Swift bridge
  (`networkframework` → `apple-cf`), so it runs *impure* (`__noChroot`) against Xcode's toolchain
  and SDK. See [Building notes](#building-notes-impurity-and-caching) — this is the one real wrinkle.
- **Remote Login (sshd) enabled.** In this config `modules/darwin/power.nix` already runs
  `systemsetup -setremotelogin on`, so sshd listens on `127.0.0.1:22`. If yours doesn't, enable it
  (System Settings → General → Sharing → Remote Login).
- The same login user on each host (`pplanel` here; shown as `${user}` below).

## Topology

```
          arrakis ─┐         each host:
                   │           • runs airshell-proxy  → advertises _awdlssh._tcp as its hostname
          caladan ─┼─ AWDL      • relays inbound AWDL streams to local sshd (127.0.0.1:22)
                   │           • ssh config resolves the OTHER two by name via airshell-connect
         imperium ─┘
```

A proxy with no `--name` advertises under the computer name *inside* Network.framework, which this
process can't read back for logging. We pass `--name <hostname>` explicitly so the advertised
Bonjour name is deterministic and matches the `ProxyCommand` on the other hosts.

## Step 1 — add airshell as a flake input

In `flake.nix`, add the input alongside your other `github:pplanel/*` flakes:

```nix
# airshell: SSH (and any TCP service) over AWDL between nearby Macs.
# NOTE: intentionally NOT following nixpkgs — airshell is served by its own GHCR
# cache (ghcr.io/pplanel/airshell/nix-cache), so it must build with its own
# pinned nixpkgs for the store path to match what CI published; following
# nixpkgs would force a local rebuild (impure, needs Xcode) instead of
# substituting. Same pattern as neovix/pam-watchid.
airshell.url = "github:pplanel/airshell";
```

`inputs` is already threaded into every host via `specialArgs = { inherit inputs self user; }`, so
the module in the next step can reach `inputs.airshell` without further plumbing.

## Step 2 — the airshell module

Create `modules/darwin/airshell.nix`. It installs both binaries, runs the proxy as a per-user
LaunchAgent, pre-approves the binary in the application firewall, and generates the SSH client
config for the peers — all keyed off `config.networking.hostName`, so the *same module* does the
right thing on each host.

```nix
{
  config,
  pkgs,
  lib,
  inputs,
  user,
  ...
}: let
  airshell = inputs.airshell.packages.${pkgs.stdenv.hostPlatform.system};
  proxy = "${airshell.airshell-proxy}/bin/airshell-proxy";
  connect = "${airshell.airshell-connect}/bin/airshell-connect";

  # Every host in the mesh. The advertised Bonjour name == the hostname.
  allHosts = ["arrakis" "caladan" "imperium"];
  self = config.networking.hostName;
  peers = lib.filter (h: h != self) allHosts;

  logDir = "/Users/${user}/Library/Logs";
in {
  # Both binaries on PATH, so `airshell-connect <host>` works from a shell too.
  environment.systemPackages = [airshell.airshell-proxy airshell.airshell-connect];

  # Run the proxy in the user's GUI (Aqua) session, not as a root daemon:
  # AWDL + the "Local Network" privacy grant are per-user and live in that
  # session. One advertisement for sshd, restarted if it dies.
  launchd.user.agents.airshell-proxy = {
    serviceConfig = {
      ProgramArguments = [proxy "--name" self "--port" "22"];
      RunAtLoad = true;
      KeepAlive = true;
      StandardOutPath = "${logDir}/airshell-proxy.out.log";
      StandardErrorPath = "${logDir}/airshell-proxy.err.log";
      EnvironmentVariables.RUST_LOG = "info";
    };
  };

  # The firewall (enabled on e.g. imperium) blocks unsolicited inbound until the
  # listener is allowed. The store path changes each build, so (re-)approve it on
  # every activation. The per-user "Local Network" prompt still has to be accepted
  # once interactively — that one can't be scripted.
  system.activationScripts.postActivation.text = lib.mkAfter ''
    fw=/usr/libexec/ApplicationFirewall/socketfilterfw
    if [ -x "$fw" ]; then
      "$fw" --add ${proxy}       >/dev/null 2>&1 || true
      "$fw" --unblockapp ${proxy} >/dev/null 2>&1 || true
    fi
  '';

  # ssh <peer> dials that peer's proxy over AWDL. airshell-connect writes only to
  # stderr/its log — stdout stays the ssh data channel. `enable = true` is what
  # makes home-manager actually render ~/.ssh/config.
  home-manager.users.${user}.programs.ssh = {
    enable = true;
    matchBlocks = lib.genAttrs peers (h: {
      user = user;
      proxyCommand = "${connect} ${h}";
    });
  };
}
```

> **Heads-up — home-manager takes over `~/.ssh/config`.** Enabling `programs.ssh` makes
> home-manager render the *entire* `~/.ssh/config`. If you already hand-maintain that file, the
> first switch either aborts ("would be clobbered") or backs it up to `~/.ssh/config.bak` and drops
> your manual hosts. Two ways through:
>
> 1. **Migrate** your existing `Host` entries into `programs.ssh.matchBlocks` (the idiomatic route;
>    merges cleanly with the `genAttrs` above).
> 2. **Don't let HM own the file** — write just the airshell entries to a side file and `Include`
>    it from your existing config. Replace the `programs.ssh` block with:
>    ```nix
>    home-manager.users.${user}.home.file.".ssh/airshell.conf".text =
>      lib.concatMapStrings (h: ''
>        Host ${h}
>            User ${user}
>            ProxyCommand ${connect} ${h}
>      '') peers;
>    ```
>    then add `Include ~/.ssh/airshell.conf` once at the top of your `~/.ssh/config`.

## Step 3 — import it on every host

Add the module to the shared import list in `hosts/darwin/default.nix` (it's imported by all three
hosts through `mkDarwinHost`):

```nix
imports = [
  # …existing modules…
  ../../modules/darwin/power.nix
  ../../modules/darwin/airshell.nix   # ← add
  ../../modules/darwin/home-manager.nix
  # …
];
```

That's all three hosts configured. If you'd rather roll it out one host at a time, drop the import
here and instead add `../../modules/darwin/airshell.nix` to each host's `extraModules` in `flake.nix`
as you go.

## Step 4 — build and switch

On each host:

```bash
darwin-rebuild switch --flake ~/.config/nix-darwin#$(hostname -s)
# or your wrapper: nix run ~/.config/nix-darwin#build-switch
```

The first build on a host compiles airshell (Xcode required — see below). Subsequent hosts with the
same store path substitute it if you've wired a cache.

**Accept the one-time prompts** (per host, per user):

- **Local Network** — the first time the agent advertises, macOS asks to let the app use the local
  network. Accept it, or the Bonjour advertisement is silent and peers can't find the host. This is
  a per-user TCC grant and cannot be set declaratively.
- If `airshell-proxy` never logs `listener ready`, it's usually waiting on this prompt or the
  firewall. Run it once from a local Terminal to surface the dialog:
  `airshell-proxy --name $(hostname -s)`.

## Step 5 — verify

```bash
# The agent is loaded and advertising:
launchctl print gui/$(id -u)/application.*.airshell-proxy 2>/dev/null | grep -i state
tail -f ~/Library/Logs/airshell-proxy.err.log
#  INFO airshell_proxy: listener ready host=127.0.0.1 port=22
#  INFO airshell_proxy: broadcasting name=caladan service=_awdlssh._tcp domain=local.

# From caladan, off every network, reach the others over AWDL:
ssh arrakis
ssh imperium

# Watch the traffic ride the AWDL interface:
netstat -I awdl0 -b 1
```

`scp` and `rsync` to `arrakis`/`caladan`/`imperium` work the same way, through the same
`ProxyCommand`. `airshell-connect` retries for ~30 s while a peer is still being discovered.

## Serving something other than ssh

The proxy relays any local TCP service; the SSH case is just `--port 22`. To expose, say, Postgres
on one host, run a second agent (or switch to a config file — see the project README's
["Running several services from one proxy"](../README.md)) with a distinct `--service-type`, e.g.
`--service-type _awdldb._tcp --port 5432`, and resolve it on the client with the matching
`--service-type`. AWDL has no authentication, so only expose services that authenticate themselves
(ssh does); that's exactly why ssh is the first application.

## Building notes: impurity and caching

airshell's flake build is **impure** (`__noChroot = true`): it drops the Nix SDK and uses Xcode's
`clang`/`swift`/`xcrun` plus the macOS 26 SDK, because `networkframework`'s `build.rs` compiles a
Swift bridge. Consequences for this setup:

- **Xcode must be installed on any host that builds it.** A host that only *substitutes* a prebuilt
  binary needs no Xcode.
- The building user must be a **trusted Nix user** (this config already sets
  `trusted-users = root ${user} @admin`) so the `__noChroot` derivation is permitted.
- To avoid compiling on all three Macs, serve airshell from a binary cache the way this repo already
  does for `neovix` and `pam-watchid` (`modules/darwin/nixcache.nix`): build once, push to
  `ghcr.io/pplanel/airshell/nix-cache`, and add a `nixcache` entry so the other hosts substitute it.
  Until then, each host compiles on first switch (a few minutes) and reuses the result after.

## Why a user agent, not a root daemon

`launchd.user.agents` runs the proxy inside the logged-in user's Aqua session. AWDL interfaces and
the "Local Network" privacy grant are scoped to that session; a root `launchd.daemons` service would
advertise from a context where the grant and the interface may not be available, and its logs
wouldn't land in the user's `~/Library/Logs`. The trade-off is that the proxy only runs while the
user is logged in — fine for interactive machines, and the `KeepAlive` restarts it across crashes.
```
