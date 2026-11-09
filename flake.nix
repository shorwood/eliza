# Entry point for `nix develop`, `nix build`, and `nix flake check`.
# The build recipes live under nix/; this file chooses what each platform exposes.
{
  description = "1966 ELIZA through AI provider-compatible HTTP APIs";

  # --- Pin external inputs so local and CI builds resolve the same dependencies.
  # `follows` makes Fenix reuse our nixpkgs input instead of
  # pulling separate copies into the dependency graph.
  # The separate Darwin pins retain x86_64 macOS support for release downloads.
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
  };

  # outputs is a function of the pinned inputs. Its returned attribute sets
  # become targets such as .#eliza, .#dockerImage, and `nix develop`.
  outputs =
    {
      self,
      fenix,
      fenixDarwin,
      nixpkgs,
      nixpkgsDarwin,
      ...
    }:
    let
      # --- Separate development platforms from release-only platforms.
      # Intel macOS produces a download, but has no exposed package or dev shell.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      releaseSystems = systems ++ [ "x86_64-darwin" ];
      # genAttrs calls a function once per system and keys the result by system.
      forAllSystems = nixpkgs.lib.genAttrs systems;
      forReleaseSystems = nixpkgs.lib.genAttrs releaseSystems;
      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package.version;

      # --- Assemble the reusable parts of one platform's build.
      # callPackage fills ordinary dependencies from nixpkgs; we supply project
      # values such as the pinned toolchain and the fetched Scalar script.
      componentsFor =
        system:
        let
          pkgs =
            if system == "x86_64-darwin" then
              nixpkgsDarwin.legacyPackages.${system}
            else
              nixpkgs.legacyPackages.${system};
          scalarJs = pkgs.callPackage ./nix/scalar.nix { };
          rustToolchain =
            (fenix.packages.${system}.toolchainOf {
              channel = "nightly";
              date = "2026-04-16";
              sha256 = "1kbgfwz1hd2z6fkq97650in4fwa13y9n6rhjankh5hbmis8mqkn3";
            }).withComponents
              [
                "cargo"
                "clippy"
                "rustc"
                "rustfmt"
              ];
          package = pkgs.callPackage ./nix/package.nix {
            inherit rustToolchain scalarJs;
            supportedSystems = systems;
          };
          release = import ./nix/release {
            inherit
              package
              pkgs
              scalarJs
              system
              version
              ;
            fenixPackages = (if system == "x86_64-darwin" then fenixDarwin else fenix).packages.${system};
          };
        in
        # `//` merges attribute sets, adding the normal package and development
        # shell to the outputs assembled by nix/release/.
        release
        // {
          inherit package;
          devShell = pkgs.callPackage ./nix/dev/shell.nix {
            inherit rustToolchain scalarJs;
          };
        };
    in
    {
      # --- Expose downloads and platform-specific build targets.
      # optionalAttrs omits unsupported outputs instead of publishing nulls.
      # Nix evaluates values lazily, so unused components are not built.
      packages = forReleaseSystems (
        system:
        let
          components = componentsFor system;
        in
        {
          inherit (components) releaseAssets releaseCheck;
        }
        // nixpkgs.lib.optionalAttrs (system != "x86_64-darwin") {
          eliza = components.package;
          default = components.package;
        }
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "-linux" system) {
          inherit (components) dockerImage;
        }
        // nixpkgs.lib.optionalAttrs (system == "x86_64-linux") {
          inherit (components) releaseWindowsExecutable;
        }
      );

      # --- Make `nix flake check` build and inspect the published artifacts.
      # The package derivation also runs its Cargo checks during this build.
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

      # --- Provide the pinned compiler and local tooling to contributors.
      devShells = forAllSystems (system: {
        default = (componentsFor system).devShell;
      });
    };
}
