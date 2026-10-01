# Gear journal

- Rust jobs need a macOS runner: networkframework links Network.framework and builds a Swift bridge.
- Only the `xcode-27` hosted label (preview, actions/runner-images#14404) has the iOS 27 SDK;
  `macos-26` stops at Xcode 26.6 / iOS 26.5. Select Xcode with the `Xcode_27.0.app` alias via
  `DEVELOPER_DIR`; the image's real app path has changed between rollouts. actionlint needs the
  label in `.github/actionlint.yaml` until it learns it.
- Pinning actions: dereference annotated tags to the commit (`git/tags/<sha>`); e.g.
  Swatinem/rust-cache v2.9.2's tag object is not its commit.
- Networked tests are `#[ignore]`d and run on a test Mac via scripts/remote-test.sh; CI runs only
  the in-memory ones.
