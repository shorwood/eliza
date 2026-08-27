{
  description = "ELIZA compatibility server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    dylint-src.url = "github:trailofbits/dylint/v6.0.1";
    dylint-src.flake = false;
  };

  outputs = { self, nixpkgs, fenix, dylint-src, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      packageFor = system:
        let
          pkgs = import nixpkgs {
            inherit system;
            config.allowUnfreePredicate = package: (package.pname or "") == "eliza";
          };
        in pkgs.rustPlatform.buildRustPackage {
          pname = "eliza";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = pkgs.lib.fileset.toSource {
            root = ./.;
            fileset = pkgs.lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              ./examples/rust-rig
              ./fixtures
              ./src
              ./tests
            ];
          };
          cargoLock.lockFile = ./Cargo.lock;
          meta = {
            description = "Standalone classic ELIZA server with provider-compatible HTTP APIs";
            license = {
              fullName = "Functional Source License, Version 1.1, ALv2 Future License";
              shortName = "fsl11Alv2";
              spdxId = "FSL-1.1-ALv2";
              url = "https://spdx.org/licenses/FSL-1.1-ALv2.html";
              free = false;
              redistributable = true;
            };
            mainProgram = "eliza";
            platforms = systems;
          };
        };
      devEnvironmentFor = system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          rust = pkgs.callPackage ./nix/rust-toolchain.nix { inherit fenix system; };
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rust.toolchain;
            rustc = rust.toolchain;
          };
          dylintTools = pkgs.callPackage ./nix/dylint-tools.nix {
            inherit rustPlatform;
            dylintSrc = dylint-src;
          };
          dylintDriver = pkgs.callPackage ./nix/dylint-driver.nix {
            inherit rustPlatform;
            dylintSrc = dylint-src;
            rustToolchain = rust.toolchain;
            toolchainLabel = rust.toolchainLabel;
          };
        in pkgs.mkShell {
          packages = [
            pkgs.aichat
            pkgs.curl
            pkgs.nodejs_22
            pkgs.pnpm
            rust.toolchain
            dylintTools
            pkgs.just
            pkgs.openssl
            pkgs.pkg-config
            pkgs.stdenv.cc
          ];
          RUSTC = "${rust.toolchain}/bin/rustc";
          RUSTDOC = "${rust.toolchain}/bin/rustdoc";
          RUSTUP_TOOLCHAIN = rust.toolchainLabel;
          DYLINT_DRIVER_PATH = "${dylintDriver}";
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [ pkgs.openssl pkgs.stdenv.cc.cc.lib ];
        };
    in {
      packages = forAllSystems (system:
        let package = packageFor system;
        in {
          eliza = package;
          default = package;
        });
      checks = forAllSystems (system: {
        package = self.packages.${system}.eliza;
      });
      devShells = forAllSystems (system: { default = devEnvironmentFor system; });
    };
}
