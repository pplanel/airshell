#!/usr/bin/env bash
# Build the macOS apps (airshell-proxy, airshell-connect) for macOS 26+ via Nix.
# The build uses Xcode's toolchain and SDK (see flake.nix), so Xcode must be installed.
# Prints the store paths it produced.
# Usage: scripts/build-mac.sh [nix build args...]
set -euo pipefail
cd "$(dirname "$0")/.."

nix build "$@" --no-link --print-out-paths .#airshell-proxy .#airshell-connect
