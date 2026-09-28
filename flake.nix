{
  description = "ELIZA compatibility server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    rlib.url = "git+file:../rlib";
    rlib.inputs.nixpkgs.follows = "nixpkgs";
    rlib.inputs.fenix.follows = "fenix";
  };

  outputs = { self, nixpkgs, rlib, ... }:
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
          rustToolchain = rlib.packages.${system}.rust-toolchain;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
        in rustPlatform.buildRustPackage {
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
          nativeCheckInputs = [ pkgs.hurl ];
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
        in pkgs.mkShell {
          inputsFrom = [ rlib.devShells.${system}.consumer ];
          packages = [
            pkgs.aichat
            pkgs.curl
            pkgs.hurl
            pkgs.just
            pkgs.nodejs_22
            pkgs.openssl
            pkgs.pnpm
            pkgs.pkg-config
            pkgs.stdenv.cc
          ];
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

      devShells = forAllSystems (system: {
        default = devEnvironmentFor system;
      });
    };
}
