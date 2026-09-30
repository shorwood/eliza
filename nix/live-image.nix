# Build the chat image from the live checkout while reusing locked flake inputs.
let
  flake = builtins.getFlake "git+file:${toString ../.}";
  system = builtins.currentSystem;
  pkgs = flake.inputs.nixpkgs.legacyPackages.${system};
  package = pkgs.callPackage ./package.nix {
    projectRoot = ../.;
    rustToolchain = flake.inputs.rlib.packages.${system}.rust-toolchain;
    supportedSystems = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
  };
in
pkgs.callPackage ./docker-image.nix { inherit package; }
