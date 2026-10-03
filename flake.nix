{
  description = "airshell development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    ...
  }: let
    system = "aarch64-darwin";
    pkgs = import nixpkgs {
      inherit system;
      overlays = [rust-overlay.overlays.default];
    };

    rustToolchain = pkgs.rust-bin.stable.latest.default.override {
      targets = ["aarch64-apple-ios"];
    };
    rustPlatform = pkgs.makeRustPlatform {
      cargo = rustToolchain;
      rustc = rustToolchain;
    };

    # airshell-connect and airshell-proxy, the macOS AWDL relay apps, for macOS
    # 26+. Like the devShell, cc/swift/xcrun and the macOS SDK come from Xcode, not
    # Nix: networkframework (via apple-cf) compiles a Swift bridge in build.rs that
    # needs them, so the build runs impure (__noChroot) against the host /usr/bin.
    airshellBin = bin:
      rustPlatform.buildRustPackage {
        pname = bin;
        version = "0.1.0";
        src = ./.;
        cargoLock = {
          lockFile = ./Cargo.lock;
          outputHashes = {
            "networkframework-0.14.0" = "sha256-BqlaMDdeV7fZJuq+f3u0cKlSsLNXzx4eaBdA6xTXQDQ=";
          };
        };
        cargoBuildFlags = ["--bin" bin];

        __noChroot = true;
        # Xcode's clang/swift/xcrun and its macOS 26 SDK; the Nix stdenv otherwise
        # pins SDKROOT/DEVELOPER_DIR to a bundled older SDK that can't target 26.
        preBuild = ''
          # Drop the Nix SDK the stdenv exported, or xcrun just echoes it back.
          unset SDKROOT DEVELOPER_DIR
          # Append (not prepend) /usr/bin so Xcode's xcrun/xcode-select/clang are
          # found without shadowing Nix's GNU coreutils (the install hook needs them).
          export PATH="$PATH:/usr/bin"
          export DEVELOPER_DIR="$(xcode-select -p)"
          export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
          export HOME="$TMPDIR"
          # Re-assert the min-OS; the stdenv setup hook resets it to the Nix SDK's.
          export MACOSX_DEPLOYMENT_TARGET=26.0
          # networkframework/apple-cf build.rs run `swift build`, whose manifest
          # sandbox-exec is rejected here; shim swift to pass --disable-sandbox.
          mkdir -p "$TMPDIR/swift-shim"
          cat > "$TMPDIR/swift-shim/swift" <<'EOF'
          #!/bin/sh
          if [ "$1" = build ]; then shift; exec /usr/bin/swift build --disable-sandbox "$@"; fi
          exec /usr/bin/swift "$@"
          EOF
          chmod +x "$TMPDIR/swift-shim/swift"
          export PATH="$TMPDIR/swift-shim:$PATH"
          # Link with Xcode's clang, not the Nix cc wrapper (which targets the Nix
          # SDK, can't see the Xcode Swift runtime, and forces an older min-OS).
          # The static Swift bridge auto-links the Swift runtime, so point ld at
          # the Xcode SDK (-isysroot resolves its .tbd stubs) and the runtime dirs.
          export RUSTFLAGS="''${RUSTFLAGS:-} \
            -C linker=/usr/bin/clang \
            -C link-arg=-isysroot -C link-arg=$SDKROOT \
            -C link-arg=-L$SDKROOT/usr/lib/swift \
            -C link-arg=-L$DEVELOPER_DIR/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx"
        '';
        doCheck = false;
      };
  in {
    packages.${system} = {
      airshell-connect = airshellBin "airshell-connect";
      airshell-proxy = airshellBin "airshell-proxy";
    };

    apps.${system} = {
      airshell-connect = {
        type = "app";
        program = "${self.packages.${system}.airshell-connect}/bin/airshell-connect";
      };
      airshell-proxy = {
        type = "app";
        program = "${self.packages.${system}.airshell-proxy}/bin/airshell-proxy";
      };
    };

    # NoCC: no Nix C toolchain or macOS SDK in the environment, so cc, swift
    # and xcrun come from Xcode, which has the iPhoneOS SDK.
    devShells.${system}.default = pkgs.mkShellNoCC {
      packages = [
        rustToolchain
        pkgs.jq # scripts/remote-test.sh
      ];
      # Wi-Fi Aware, and the networkframework bridge, need iOS 26.
      IPHONEOS_DEPLOYMENT_TARGET = "26.0";
      # Xcode's clang: a Nix cc wrapper on PATH only targets macOS.
      CARGO_TARGET_AARCH64_APPLE_IOS_LINKER = "/usr/bin/clang";
    };
  };
}
