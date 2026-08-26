{
  description = "ELIZA compatibility server development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix.url = "github:nix-community/fenix";
    fenix.inputs.nixpkgs.follows = "nixpkgs";
    dylint-src.url = "github:trailofbits/dylint/v6.0.1";
    dylint-src.flake = false;
  };

  outputs = { nixpkgs, fenix, dylint-src, ... }:
    let
      forAllSystems = nixpkgs.lib.genAttrs [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
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
      devShells = forAllSystems (system: { default = devEnvironmentFor system; });
    };
}
