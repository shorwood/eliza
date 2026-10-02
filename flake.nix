{
  description = "ELIZA compatibility server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rlib = {
      url = "github:shorwood/rlib/v1.0.0";
      inputs = {
        nixpkgs.follows = "nixpkgs";
        fenix.follows = "fenix";
      };
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rlib,
      ...
    }:
    let
      # Keep supported hosts in one place so every output exposes the same set.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      # Component files are ordinary Nix functions. `callPackage` supplies
      # dependencies from nixpkgs; only project-specific values are passed here.
      componentsFor =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          package = pkgs.callPackage ./nix/package.nix {
            rustToolchain = rlib.packages.${system}.rust-toolchain;
            supportedSystems = systems;
          };
        in
        {
          inherit package;
          dockerImage = pkgs.callPackage ./nix/docker-image.nix { inherit package; };
          devShell = pkgs.callPackage ./nix/dev-shell.nix {
            rlibShell = rlib.devShells.${system}.consumer;
          };
        };
    in
    {
      packages = forAllSystems (
        system:
        let
          components = componentsFor system;
        in
        {
          eliza = components.package;
          default = components.package;
        }
        # Containers always run Linux binaries. A non-Linux host can still
        # request one of these outputs when it has a Linux Nix builder.
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "-linux" system) {
          inherit (components) dockerImage;
        }
      );

      # Flake checks build the artifacts themselves; Cargo's checks run as part
      # of the Rust package derivation.
      checks = forAllSystems (
        system:
        {
          package = self.packages.${system}.eliza;
        }
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "-linux" system) {
          dockerImage = self.packages.${system}.dockerImage;
        }
      );

      devShells = forAllSystems (system: {
        default = (componentsFor system).devShell;
      });
    };
}
