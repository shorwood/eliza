{
  description = "1966 ELIZA through AI provider-compatible HTTP APIs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    nixpkgsDarwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    fenixDarwin = {
      url = "github:nix-community/fenix/1635a532f6778304adfac2044c82992ece8a99d8";
      inputs.nixpkgs.follows = "nixpkgsDarwin";
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
      fenix,
      fenixDarwin,
      nixpkgs,
      nixpkgsDarwin,
      rlib,
      ...
    }:
    let
      # Intel macOS has a release archive, but no dev shell or Nix package.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      releaseSystems = systems ++ [ "x86_64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      forReleaseSystems = nixpkgs.lib.genAttrs releaseSystems;
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;
      # Match the nightly pinned by rlib, adding target std for portable builds.
      releaseRust = {
        channel = "nightly";
        date = "2026-04-16";
        sha256 = "1kbgfwz1hd2z6fkq97650in4fwa13y9n6rhjankh5hbmis8mqkn3";
      };
      releaseToolchainFor =
        system: target:
        let
          tools = (if system == "x86_64-darwin" then fenixDarwin else fenix).packages.${system};
          dated = tools.toolchainOf releaseRust;
        in
        if target == null then
          dated.withComponents [
            "cargo"
            "rustc"
          ]
        else
          tools.combine [
            dated.cargo
            dated.rustc
            (tools.targets.${target}.toolchainOf releaseRust).rust-std
          ];

      # Component files are ordinary Nix functions. `callPackage` supplies
      # dependencies from nixpkgs; only project-specific values are passed here.
      componentsFor =
        system:
        let
          pkgs =
            if system == "x86_64-darwin" then
              nixpkgsDarwin.legacyPackages.${system}
            else
              nixpkgs.legacyPackages.${system};
          scalarJs = pkgs.callPackage ./nix/scalar.nix { };
          package = pkgs.callPackage ./nix/package.nix {
            rustToolchain = rlib.packages.${system}.rust-toolchain;
            inherit scalarJs;
            supportedSystems = systems;
          };
          linux = nixpkgs.lib.hasSuffix "-linux" system;
          arch = if nixpkgs.lib.hasPrefix "x86_64-" system then "x86_64" else "arm64";
          releaseToolchain = releaseToolchainFor system (
            if linux then "${if arch == "x86_64" then "x86_64" else "aarch64"}-unknown-linux-musl" else null
          );
          portable = (if linux then pkgs.pkgsStatic else pkgs).callPackage ./nix/release-binary.nix {
            inherit scalarJs;
            rustToolchain = releaseToolchain;
          };
          releaseArchive = pkgs.callPackage ./nix/release-archive.nix {
            binary = portable;
            inherit version;
            platform = "${if linux then "linux" else "macos"}-${arch}";
          };
        in
        rec {
          inherit package releaseArchive;
          dockerImage = pkgs.callPackage ./nix/docker-image.nix { inherit package; };
          releaseDeb = pkgs.callPackage ./nix/release-deb.nix {
            binary = portable;
            inherit version;
            arch = if arch == "x86_64" then "amd64" else "arm64";
          };
          releaseApk = pkgs.callPackage ./nix/release-apk.nix {
            binary = portable;
            inherit version;
            arch = if arch == "x86_64" then "x86_64" else "aarch64";
          };
          releaseWindowsArchive = pkgs.callPackage ./nix/release-archive.nix {
            binary = pkgs.pkgsCross.mingwW64.callPackage ./nix/release-binary.nix {
              inherit scalarJs;
              rustToolchain = releaseToolchainFor system "x86_64-pc-windows-gnu";
            };
            inherit version;
            platform = "windows-x86_64";
            isWindows = true;
          };
          releaseCheck = pkgs.callPackage ./nix/release-check.nix {
            archive = releaseArchive;
            deb = if linux then releaseDeb else null;
            apk = if linux then releaseApk else null;
            isLinux = linux;
          };
          devShell = pkgs.callPackage ./nix/dev-shell.nix {
            rlibShell = rlib.devShells.${system}.consumer;
            inherit scalarJs;
          };
        };
    in
    {
      packages = forReleaseSystems (
        system:
        let
          components = componentsFor system;
        in
        {
          inherit (components) releaseArchive releaseCheck;
        }
        // nixpkgs.lib.optionalAttrs (system != "x86_64-darwin") {
          eliza = components.package;
          default = components.package;
        }
        # Containers always run Linux binaries. A non-Linux host can still
        # request one of these outputs when it has a Linux Nix builder.
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "-linux" system) {
          inherit (components) dockerImage releaseDeb releaseApk;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          inherit (components) releaseWindowsArchive;
        }
      );

      # Flake checks build the artifacts themselves; Cargo's checks run as part
      # of the Rust package derivation.
      checks = forReleaseSystems (
        system:
        {
          release = (componentsFor system).releaseCheck;
        }
        // nixpkgs.lib.optionalAttrs (system != "x86_64-darwin") {
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
