#!/usr/bin/env bash
# Build the airshell library (with Wi-Fi Aware) as a static library for iOS devices, for an
# app that also links swift/AirshellWiFiAware. Run inside `nix develop` (needs the iOS target).
# Prints the frameworks the app must link.
# Usage: scripts/build-ios.sh [cargo args...]
set -euo pipefail
cd "$(dirname "$0")/.."
export IPHONEOS_DEPLOYMENT_TARGET=${IPHONEOS_DEPLOYMENT_TARGET:-26.0}

cargo rustc --lib --release --target aarch64-apple-ios --features wifi-aware --crate-type staticlib \
  "$@" -- --print native-static-libs
echo "built target/aarch64-apple-ios/release/libairshell.a"
