{
  inputs = {
    nixpkgs = {
      url = "github:nixos/nixpkgs/nixos-unstable";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    dioxus-cli-gh = {
      url = "github:DioxusLabs/dioxus/v0.7.10";
      inputs.nixpkgs.follows = "nixpkgs"; # Use same nixpkgs
      inputs.rust-overlay.follows = "rust-overlay"; # Crucial: inherit the overlay
      inputs.flake-parts.inputs.nixpkgs-lib.follows = "nixpkgs";
    };
  };
  outputs =
    inputs@{
      self,
      nixpkgs,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      eachSystem = nixpkgs.lib.genAttrs systems;

      perSystem =
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
            libX11
            libxcb
            zlib
            sqlite
          ];

          runtimeDeps = with pkgs; [
          ];

          surrealdb-bin = pkgs.stdenv.mkDerivation rec {
            pname = "surrealdb";
            version = "3.3.0";
            src = pkgs.fetchurl {
              url = "https://github.com/surrealdb/surrealdb/releases/download/v${version}/surreal-v${version}.linux-amd64.tgz";
              hash = "sha256-RK6rVl9+fjnS0L8Fg8iq5ki6vJE3PHCyZYKI2Vu7zVU=";
            };
            sourceRoot = ".";
            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [
              pkgs.stdenv.cc.cc.lib
              pkgs.openssl
            ];
            installPhase = ''
              mkdir -p $out/bin
              cp surreal $out/bin/
              chmod +x $out/bin/surreal
            '';
          };

          wasm-bindgen-cli-bin =
            let
              targets = {
                "x86_64-linux" = {
                  target = "x86_64-unknown-linux-musl";
                  sha256 = "82d12bb940e2d4e72e0d5605387fc1b8ca179044e012b620f0ce4e7440e8320e";
                };
                "aarch64-linux" = {
                  target = "aarch64-unknown-linux-musl";
                  sha256 = "2ed4351c35dd9440308bbb02767d47ea278efe851a52465300f3c94f5b6c2a87";
                };
                "aarch64-darwin" = {
                  target = "aarch64-apple-darwin";
                  sha256 = "81d4a23d56b3c3eb8187658329116d50e0b228a93b343825fb71f70179051cd1";
                };
                "x86_64-darwin" = {
                  target = "x86_64-apple-darwin";
                  sha256 = "7e028879a68ec53dae14048b55cc5215f6df75b17a9c1426fac8fa086d3202fd";
                };
              };
              targetInfo = targets.${system} or (throw "Unsupported system for wasm-bindgen-cli: ${system}");
            in
            pkgs.stdenv.mkDerivation rec {
              pname = "wasm-bindgen-cli";
              version = "0.2.129";
              src = pkgs.fetchurl {
                url = "https://github.com/wasm-bindgen/wasm-bindgen/releases/download/${version}/wasm-bindgen-${version}-${targetInfo.target}.tar.gz";
                inherit (targetInfo) sha256;
              };
              sourceRoot = ".";
              installPhase = ''
                mkdir -p $out/bin
                find . -type f -name wasm-bindgen -exec cp {} $out/bin/ \;
                find . -type f -name wasm-bindgen-test-runner -exec cp {} $out/bin/ \;
                chmod +x $out/bin/*
              '';
            };

          # Deps shared by both nix build and devshell (linking libs, pkg-config, etc.)
          commonDeps =
            with pkgs;
            [
              # llvmPackages_21.clang-unwrapped
              pkg-config
              rustPlatform.bindgenHook
              wasm-bindgen-cli-bin
            ]
            ++ dioxusDeps
            ++ (if enableAndroid then androidDeps else [ ]);

          # Deps only needed for nix build (not the devshell)
          buildOnlyDeps = [ ];

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
              esbuild
              fish
              gdb
              just
              nushell
              panamax
              tailwindcss
              zellij
            ]
            ++ [
              surrealdb-bin
              inputs.dioxus-cli-gh.packages.${system}.dioxus-cli
            ];

          cargoToml = builtins.fromTOML (builtins.readFile ./ynab-payee-rs/Cargo.toml);
          msrv = cargoToml.package.rust-version or "1.80.0";

          rustPackage =
            features:
            (pkgs.makeRustPlatform {
              cargo = pkgs.rust-bin.stable.latest.minimal;
              rustc = pkgs.rust-bin.stable.latest.minimal;
            }).buildRustPackage
              {
                inherit (cargoToml.package) name version;
                src = ./ynab-payee-rs;
                cargoLock.lockFile = ./ynab-payee-rs/Cargo.lock;
                buildFeatures = features;
                buildInputs = runtimeDeps;
                nativeBuildInputs = commonDeps ++ buildOnlyDeps;
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
                # Prevent argument list too long when compiling host build scripts
                unset NIX_LDFLAGS NIX_LDFLAGS_FOR_BUILD

                export SHELL="${pkgs.fish}/bin/fish"

                # 1. [[ $- == *i* ]] checks if the CURRENT execution shell context is interactive.
                # 2. [ -z "$i" ] guards against some edge-case nested script evaluation loops.
                if [[ $- == *i* ]] && [ -z "$i" ]; then
                  export i=1
                  exec $SHELL -i
                fi
              '';
              LD_LIBRARY_PATH = lib.makeLibraryPath ldpath;

              # WASM cross-compilation needs the unwrapped clang: the Nix wrapper
              # rejects --target flags that C build scripts may pass. Native
              # targets get the wrapped compiler from the stdenv.
              CC_wasm32_unknown_unknown = "${pkgs.llvmPackages_21.clang-unwrapped}/bin/clang";
              CXX_wasm32_unknown_unknown = "${pkgs.llvmPackages_21.clang-unwrapped}/bin/clang++";
              AR_wasm32_unknown_unknown = "${pkgs.llvmPackages_21.llvm}/bin/llvm-ar";

              ANDROID_HOME = if enableAndroid then "${androidSdk}/libexec/android-sdk" else "";
              ANDROID_NDK_HOME = if enableAndroid then "${androidSdk}/libexec/android-sdk/ndk-bundle" else "";

              GIO_MODULE_DIR = "${pkgs.glib-networking}/lib/gio/modules/";

              RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
              buildInputs = runtimeDeps;
              nativeBuildInputs = commonDeps ++ devDeps ++ [ rustc ];
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
        };
    in
    {
      packages = eachSystem (system: (perSystem system).packages);
      devShells = eachSystem (system: (perSystem system).devShells);
    };
}
