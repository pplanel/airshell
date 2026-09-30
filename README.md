### airshell: Zero-Infrastructure Peer-to-Peer SSH over AWDL

**airshell** explores off-grid, low-latency peer-to-peer networking between Macs by tapping directly into **Apple Wireless Direct Link (AWDL)**—the proprietary Wi-Fi mesh protocol that powers AirDrop, AirPlay, and Sidecar.

With zero shared infrastructure (no local Wi-Fi router, no internet access, and no cables), two Macs within physical proximity can discover each other, negotiate an ad-hoc connection, and establish secure SSH and SCP sessions.

---

### The Novelty: Harnessing Apple's Mesh Protocol from Rust

AWDL allows Apple hardware to communicate directly across dynamic Wi-Fi channels while remaining connected to standard networks. While traditionally reserved for closed Apple ecosystem features, macOS exposes peer-to-peer capabilities through `Network.framework`.

`airshell` bridges these native low-level Darwin capabilities into Rust:

- **Zero-Conf Mesh Discovery:** Advertises and browses specialized Bonjour records (`_awdlssh._tcp`) over peer-to-peer transport without broadcast pollution on regular networks.

- **Direct P2P Link Negotiation:** Leverages native `NWParameters` with peer-to-peer flags enabled, causing macOS to automatically negotiate ad-hoc Wi-Fi channel sync and link-state changes between machines.
- **Transparent SSH Integration:** Plugs straight into standard OpenSSH via `ProxyCommand`, enabling standard keys, config profiles, interactive shells, and high-throughput file transfers (`scp`/`rsync`) over the air.

---

### Architecture & Components

The project consists of two lightweight binaries:

```
[ Mac A (Client) ]                                       [ Mac B (Host) ]
ssh client                                               sshd (127.0.0.1:22)
    │ (stdin/stdout)                                              ▲
    ▼                                                             │ (loopback TCP)
airshell-connect ──[ AWDL Peer-to-Peer Link (_awdlssh._tcp) ]──▶ airshell-sshd
```

- **`airshell-sshd` (The Daemon):** Listens on a dynamic port advertised over AWDL peer-to-peer Bonjour and proxies incoming connections to the local OpenSSH daemon on `127.0.0.1:22`.
- **`airshell-connect <PeerName>` (The Proxy):** Acts as an SSH `ProxyCommand`. It discovers the target Mac by its Bonjour computer name, negotiates the direct peer-to-peer channel, and bridges `stdin`/`stdout`.

---

### Quick Start

#### 1. Build

```bash
cargo build --release
```

#### 2. Configure SSH Client (`~/.ssh/config`)

Add a profile for the target machine using its macOS Computer Name (as shown in **System Settings > General > About**):

```ssh-config
Host mesh-target
    HostName target.local
    User your_username
    ProxyCommand /usr/local/bin/airshell-connect "Target-MacBook"
```

#### 3. Start the Daemon on the Remote Host

Run the daemon on the host machine:

```bash
airshell-sshd
```

You should see:

```text
listener: ready
Broadcasting as: Target-MacBook [_awdlssh._tcplocal.]
```

#### 4. Connect Off-Grid

Disconnect both Macs from Wi-Fi access points and unplug Ethernet cables (keep Wi-Fi turned **on** in Control Center).

From the client Mac:

```bash
ssh mesh-target
```

To verify data is actually routing over the physical AWDL interface:

```bash
# Watch interface statistics on the AWDL link
netstat -I awdl0 -b 1
```

---

### Under the Hood

#### Peer-to-Peer Protocol Parameters

Standard TCP sockets do not trigger Apple's AWDL radio negotiation. `airshell` interfaces with Apple's `Network.framework` via the `networkframework` crate. By explicitly enabling peer-to-peer routing in the protocol stack:

- The Bonjour registration triggers background AWDL frame scheduling.
- The connecting client resolves the `_awdlssh._tcp` record directly from peer beacon frames.
- macOS handles channel synchronization, power management, and link-layer encryption under the hood.

#### Bidirectional Relay Engine

`airshell-sshd` avoids touching the public network stack entirely. It operates as a local boundary:

- Inbound connections are accepted over the virtual P2P socket.
- The daemon establishes a loopback stream to `127.0.0.1:22`.
- Full-duplex asynchronous threads pipe data between the local SSH daemon and the remote peer, ensuring minimal latency and native throughput.

---

### Operational Notes & Troubleshooting

#### macOS Application Firewall

The first time a new binary starts listening on network sockets, the macOS Application Firewall (`socketfilterfw`) will prompt for confirmation:

- An unanswered dialog holds incoming connections in a `waiting` state, causing connection timeouts.
- In automated or headless setups, pre-approve the binary (requires `sudo`):
  ```bash
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add /path/to/airshell-sshd
  sudo /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp /path/to/airshell-sshd
  ```

#### Local Network Privacy

On modern macOS releases, access to local discovery protocols requires explicit consent. If the daemon gets stuck at `listener: waiting(...)` when launched remotely over SSH, run `airshell-sshd` once inside a local Terminal session and accept the prompt.

#### Binary Replacement & Code Signature Caches

Replacing an active binary directly can cause macOS to terminate the process with `SIGKILL` due to signature validation cache mismatches. When deploying updates, write to a temporary file and atomically rename it into place:

```bash
cp target/release/airshell-sshd /usr/local/bin/airshell-sshd.new
mv -f /usr/local/bin/airshell-sshd.new /usr/local/bin/airshell-sshd
```

---

### Repository Structure

| Path                      | Purpose                                                            |
| ------------------------- | ------------------------------------------------------------------ |
| `src/lib.rs`              | Protocol constants and peer-to-peer `Network.framework` parameters |
| `src/relay.rs`            | Bidirectional full-duplex I/O streaming pipelines                  |
| `src/bin/airshell-sshd.rs`    | P2P Bonjour advertiser & loopback relay daemon                     |
| `src/bin/airshell-connect.rs` | Peer resolver and SSH `ProxyCommand` transport                     |
| `tests/`                  | P2P integration and loopback communication test suites             |
