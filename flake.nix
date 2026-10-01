{
  description = "airshell development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      pkgs = import nixpkgs {
        system = "aarch64-darwin";
        overlays = [ rust-overlay.overlays.default ];
      };
    in
    {
      # NoCC: no Nix C toolchain or macOS SDK in the environment, so cc, swift
      # and xcrun come from Xcode, which has the iPhoneOS SDK.
      devShells.aarch64-darwin.default = pkgs.mkShellNoCC {
        packages = [
          (pkgs.rust-bin.stable.latest.default.override {
            targets = [ "aarch64-apple-ios" ];
          })
          pkgs.jq # scripts/remote-test.sh
        ];
        # Wi-Fi Aware, and the networkframework bridge, need iOS 26.
        IPHONEOS_DEPLOYMENT_TARGET = "26.0";
        # Xcode's clang: a Nix cc wrapper on PATH only targets macOS.
        CARGO_TARGET_AARCH64_APPLE_IOS_LINKER = "/usr/bin/clang";
      };
    };
}
