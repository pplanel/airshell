#!/usr/bin/env bash
# Build the tests on this Mac and run them on <host> over ssh. Nothing runs locally.
# Usage: scripts/remote-test.sh <host>
# shellcheck disable=SC2029 # $remote and $name are meant to expand on this side.
set -euo pipefail
host=${1:?usage: scripts/remote-test.sh <host>}
cd "$(dirname "$0")/.."

cargo build --bins
tests=$(cargo test --no-run --message-format=json \
  | jq -r 'select(.reason == "compiler-artifact" and .profile.test == true and .executable != null) | .executable')
bins=$(ls target/debug/airshell-sshd target/debug/airshell-connect 2>/dev/null || true)

remote=/tmp/airshell-test
ssh "$host" "rm -rf $remote && mkdir -p $remote"
# shellcheck disable=SC2086 # paths contain no spaces
scp -q $tests $bins "$host:$remote/"
# scp over SFTP (OpenSSH 9+) does not keep the execute bit.
ssh "$host" "chmod +x $remote/*"

status=0
for t in $tests; do
  name=$(basename "$t")
  echo "== $name on $host"
  ssh "$host" "cd $remote && AIRSHELL_BIN_DIR=$remote ./$name --include-ignored --test-threads=1" || status=1
done
exit $status
