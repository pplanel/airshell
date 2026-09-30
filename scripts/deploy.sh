#!/usr/bin/env bash
# Build release binaries on this Mac and install them in ~/.local/bin on each given host.
# Usage: scripts/deploy.sh <host> [host...]
set -euo pipefail
cd "$(dirname "$0")/.."
[ $# -gt 0 ] || { echo "usage: scripts/deploy.sh <host> [host...]" >&2; exit 2; }

cargo build --release
for host in "$@"; do
  # Copy to *.new and rename into place: overwriting a binary in place keeps
  # the old code signature cached on that inode, and the kernel SIGKILLs the
  # new binary on launch ("killed", no error message).
  scp -q target/release/airshell-sshd "$host:.local/bin/airshell-sshd.new"
  scp -q target/release/airshell-connect "$host:.local/bin/airshell-connect.new"
  ssh "$host" 'cd ~/.local/bin && for b in airshell-sshd airshell-connect; do chmod +x "$b.new" && mv -f "$b.new" "$b"; done'
  echo "installed airshell-sshd, airshell-connect on $host"
done
