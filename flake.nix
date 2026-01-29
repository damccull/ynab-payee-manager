{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixos-unstable";
    };
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    surrealdb-gh.url = "github:surrealdb/surrealdb/v2.3.6";
    # dioxus-cli-gh.url = "github:DioxusLabs/dioxus/v0.7.2";
    dioxus-cli-gh = {
      url = "github:DioxusLabs/dioxus/v0.7.2";
      inputs.nixpkgs.follows = "nixpkgs"; # Use same nixpkgs
      inputs.rust-overlay.follows = "rust-overlay"; # Crucial: inherit the overlay
    };
  };
  outputs =
    inputs@{
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import inputs.rust-overlay) ];
          config = {
            allowUnfreePredicate =
              pkg:
              builtins.elem (lib.getName pkg) (
                [
                  "surrealdb"
                ]
                ++ (
                  if enableAndroid then
                    [
                      "android-sdk-tools"
                      "android-sdk-cmdline-tools"
                    ]
                  else
                    [ ]
                )
              );
            android_sdk.accept_license = true;
          };
        };

        lib = nixpkgs.lib;

        enableAndroid = false;

        androidSdk =
          let
            androidComposition = pkgs.androidenv.composeAndroidPackages {
              cmdLineToolsVersion = "13.0";
              # INFO: toolsVersion is unused because the tools package is deprecated
              # toolsVersion = "26.1.1";
              platformToolsVersion = "35.0.2";
              buildToolsVersions = [
                "34.0.0"
                "35.0.0"
              ];
              includeEmulator = true;
              emulatorVersion = "35.1.4";
              platformVersions = [
                "33"
              ];
              includeSources = false;
              includeSystemImages = true;
              systemImageTypes = [ "google_apis_playstore" ];
              abiVersions = [
                "x86_64"
                # "armeabi-v7a"
                # "arm64-v8a"
              ];
              cmakeVersions = [ "3.6.4111459" ];
              includeNDK = true;
              ndkVersions = [ "27.0.12077973" ];
              useGoogleAPIs = true;
              useGoogleTVAddOns = false;
              includeExtras = [
                "extras;google;gcm"
              ];
            };
          in
          androidComposition.androidsdk;

        androidDeps = with pkgs; [
          androidSdk
          openjdk
        ];

        dioxusDeps = with pkgs; [
          atkmm
          cairo
          fontconfig
          fribidi
          gdk-pixbuf
          glib
          glib-networking
          gtk3
          gsettings-desktop-schemas # Doesn't fix appimage bundle issue
          harfbuzz
          freetype
          libdrm
          libGL
          libgpg-error
          libsoup_3
          mesa
          openssl
          wrapGAppsHook3
          webkitgtk_4_1
          xdotool
          xorg.libX11
          xorg.libxcb
          zlib
          sqlite
        ];

        runtimeDeps = with pkgs; [
        ];

        buildDeps =
          with pkgs;
          [
            # llvmPackages_21.clang-unwrapped
            pkg-config
            rustPlatform.bindgenHook
            (wasm-bindgen-cli.overrideAttrs (oldAttrs: rec {
              version = "0.2.108";
              src = fetchCrate {
                pname = "wasm-bindgen-cli";
                version = version;
                hash = "sha256-UsuxILm1G6PkmVw0I/JF12CRltAfCJQFOaT4hFwvR8E=";
              };

              cargoDeps = rustPlatform.fetchCargoVendor {
                inherit src;
                inherit (src) pname version;
                hash = "sha256-iqQiWbsKlLBiJFeqIYiXo3cqxGLSjNM8SOWXGM9u43E=";
              };
            }))
          ]
          ++ dioxusDeps
          ++ (if enableAndroid then androidDeps else [ ]);

        devDeps =
          with pkgs;
          [
            # Libraries and programs needed for dev work; included in dev shell
            # NOT included in the nix build operation
            bacon
            bunyan-rs
            cargo-deny
            cargo-edit
            cargo-expand
            cargo-msrv
            cargo-nextest
            cargo-watch
            (cargo-whatfeatures.overrideAttrs (oldAttrs: rec {
              version = "0.9.13";
              src = fetchCrate {
                pname = "cargo-whatfeatures";
                version = "${version}";
                hash = "sha256-Nbyr7u47c6nImzYJvPVLfbqgDvzyXqR1C1tOLximuHU=";
              };

              cargoDeps = rustPlatform.fetchCargoVendor {
                inherit src;
                inherit (src) pname version;
                hash = "sha256-p95aYXsZM9xwP/OHEFwq4vRiXoO1n1M0X3TNbleH+Zw=";
              };
            }))
            fish
            gdb
            just
            nushell
            panamax
            tailwindcss
            zellij
          ]
          ++ [
            inputs.surrealdb-gh.packages.${system}.default
            inputs.dioxus-cli-gh.packages.${system}.dioxus-cli
          ];

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        msrv = cargoToml.package.rust-version;

        rustPackage =
          features:
          (pkgs.makeRustPlatform {
            cargo = pkgs.rust-bin.stable.latest.minimal;
            rustc = pkgs.rust-bin.stable.latest.minimal;
          }).buildRustPackage
            {
              inherit (cargoToml.package) name version;
              src = ./.;
              cargoLock.lockFile = ./Cargo.lock;
              buildFeatures = features;
              buildInputs = runtimeDeps;
              nativeBuildInputs = buildDeps;
              # Uncomment if your cargo tests require networking or otherwise
              # don't play nicely with the nix build sandbox:
              # doCheck = false;
            };

        ldpath =
          with pkgs;
          [
          ]
          ++ dioxusDeps;

        mkDevShell =
          rustc:
          pkgs.mkShell.override { stdenv = pkgs.clangStdenv; } {
            shellHook = ''
              # Override the path to ensure clang is before gcc
              # export PATH=${pkgs.llvmPackages_21.clang-unwrapped}/bin:$PATH
              # TODO: figure out if it's possible to remove this or allow a user's preferred shell
              exec env SHELL=${pkgs.fish}/bin/fish zellij --layout ./zellij_layout.kdl
            '';
            LD_LIBRARY_PATH = lib.makeLibraryPath ldpath;

            # Override gcc with clang. Must use unwrapped version because the wrapper does not
            # allow passing a target as an argument, breaking wasm compiles
            # CC = "${pkgs.llvmPackages_21.clang-unwrapped}/bin/clang";
            # CXX = "${pkgs.llvmPackages_21.clang-unwrapped}/bin/clang++";

            ANDROID_HOME = if enableAndroid then "${androidSdk}/libexec/android-sdk" else "";
            ANDROID_NDK_HOME = if enableAndroid then "${androidSdk}/libexec/android-sdk/ndk-bundle" else "";

            GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules/";

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            buildInputs = runtimeDeps;
            nativeBuildInputs = buildDeps ++ devDeps ++ [ rustc ];
          };

        rustTargets = [
          "x86_64-unknown-linux-gnu"
          "x86_64-linux-android"
          "aarch64-linux-android"
          "wasm32-unknown-unknown"
        ];

        rustExtensions = [
          "rust-analyzer"
          "rust-src"
        ];
      in
      rec {

        packages.default = packages.base;
        devShells.default = devShells.stable;

        packages.base = (rustPackage "");
        packages.bunyan = (rustPackage "bunyan");
        # packages.tokio-console = (rustPackage "tokio-console");

        devShells.nightly = (
          mkDevShell (
            pkgs.rust-bin.selectLatestNightlyWith (
              toolchain:
              toolchain.default.override {
                extensions = rustExtensions;
                targets = rustTargets;
              }
            )
          )
        );
        devShells.stable = (
          mkDevShell (
            pkgs.rust-bin.stable.latest.default.override {
              extensions = rustExtensions;
              targets = rustTargets;
            }
          )
        );
        devShells.msrv = (
          mkDevShell (
            pkgs.rust-bin.stable.${msrv}.default.override {
              extensions = rustExtensions;
              targets = rustTargets;
            }
          )
        );
      }
    );
}
