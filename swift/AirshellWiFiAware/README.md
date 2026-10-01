# AirshellWiFiAware

Blocking C functions over Apple's Wi-Fi Aware framework, for the Rust
`wifi_aware` module (`src/wifi_aware.rs`, cargo feature `wifi-aware`). Wi-Fi
Aware is Swift-only and iOS/iPadOS-only (iPhone 12+, recent iPads); the macOS
SDK marks it unavailable, so Macs keep using AWDL.

## Build

```bash
xcodebuild -scheme AirshellWiFiAware -destination 'generic/platform=iOS' build
```

## Using it in an iOS app

The app, not this package, provides:

- Entitlement `com.apple.developer.wifi-aware` with `Publish` and/or `Subscribe`.
- Info.plist services matching `wifi_aware::SERVICE`:
  ```xml
  <key>WiFiAwareServices</key>
  <dict>
      <key>_airshell._tcp</key>
      <dict>
          <key>Publishable</key><dict/>
          <key>Subscribable</key><dict/>
      </dict>
  </dict>
  ```
- Pairing UI: devices must be paired (DeviceDiscoveryUI's `DevicePairingView` /
  `DevicePicker`, or AccessorySetupKit) before they can connect.
- The Rust static library from `scripts/build-ios.sh` (run in `nix develop`),
  plus the frameworks that script prints.

Call the `airshell_wa_*` functions only from Rust (or other non-Swift) threads:
they block.

## Design notes

Modeled on Google Nearby's production Wi-Fi Aware medium
(`internal/platform/implementation/apple/Mediums/Aware/WiFiAwareMedium.swift`
in github.com/google/nearby):

- Connecting uses `NWBrowser` with the subscriber's descriptor and parameters
  and keeps it running until the connection is ready; finishing a
  `NetworkBrowser.run` stops the subscription the data path still needs.
- The listener's `run` handler returns right away; the `Connection` object
  keeps the `NetworkConnection` alive.
- A receiver task reads from the start, ending on `metadata.endOfStream`.

## Not yet verified on a device

- `cancel` sends a FIN with `send(Data(), endOfStream: true)` before stopping
  the receiver; Nearby closes by dropping the connection instead.
- Received data is buffered without a limit, so a slow reader loses TCP flow
  control.
- iPhone ↔ Android interop. Nearby's code works around Android firmware NAN
  pairing timing, which suggests it works, but airshell has not tried it.
