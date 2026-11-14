# Compose each platform's release outputs. Portable downloads use a separate
# binary; Linux images use the tested normal package and its Nix runtime closure.
# x86_64 Linux also cross-builds the Windows executable.
# `import ./nix/release` in flake.nix resolves to this default.nix file.
{
  fenixPackages,
  package,
  pkgs,
  scalarJs,
  system,
  version,
}:
let
  # --- Normalize the system name used in public download filenames.
  # Nix says aarch64; downloads say arm64. Package metadata uses each package
  # manager's own architecture spelling below.
  inherit (pkgs) lib;
  isLinux = pkgs.stdenv.hostPlatform.isLinux;
  arch = if lib.hasPrefix "x86_64-" system then "x86_64" else "arm64";

  # --- Pin Cargo, rustc, and target standard libraries to one Rust release.
  # Portable and cross-compiled binaries need std for their target triples;
  # Fenix lets us choose that std without changing the developer shell.
  rustVersion = {
    channel = "nightly";
    date = "2026-04-16";
    sha256 = "1kbgfwz1hd2z6fkq97650in4fwa13y9n6rhjankh5hbmis8mqkn3";
  };
  dated = fenixPackages.toolchainOf rustVersion;
  toolchainFor =
    target:
    if target == null then
      dated.withComponents [
        "cargo"
        "rustc"
      ]
    else
      fenixPackages.combine [
        dated.cargo
        dated.rustc
        (fenixPackages.targets.${target}.toolchainOf rustVersion).rust-std
      ];

  # --- Build the binary once before wrapping it in each download format.
  # pkgsStatic supplies musl-linked dependencies for Linux; macOS uses its
  # native package set. binary.nix runs no tests during this portable build.
  portable = (if isLinux then pkgs.pkgsStatic else pkgs).callPackage ./binary.nix {
    inherit scalarJs;
    rustToolchain = toolchainFor (
      if isLinux then "${if arch == "x86_64" then "x86_64" else "aarch64"}-unknown-linux-musl" else null
    );
  };

  # --- Name downloads consistently while preserving package-manager metadata.
  # runCommand creates a store output whose files CI later copies as downloads.
  releaseLinuxExecutable = pkgs.callPackage ./portable.nix {
    binary = portable;
    inherit arch version;
    platform = "linux";
  };
  releaseDeb = pkgs.callPackage ./deb.nix {
    binary = portable;
    inherit arch version;
    packageArch = if arch == "x86_64" then "amd64" else "arm64";
  };
  releaseApk = pkgs.callPackage ./apk.nix {
    binary = portable;
    inherit arch version;
    packageArch = if arch == "x86_64" then "x86_64" else "aarch64";
  };
  # macOS remains a tarball so the downloaded executable keeps its mode bits.
  releaseMacos = pkgs.callPackage ./macos.nix {
    binary = portable;
    inherit arch version;
  };

  # --- Collect downloads and verify they work on the build host.
  # symlinkJoin exposes files from several derivations under one directory;
  # CI dereferences those links when it uploads the downloadable files.
  releaseAssets = pkgs.symlinkJoin {
    name = "eliza-v${version}-${system}-assets";
    paths =
      if isLinux then
        [
          releaseLinuxExecutable
          releaseDeb
          releaseApk
        ]
      else
        [ releaseMacos ];
  };
  releaseCheck = pkgs.callPackage ./check.nix {
    assets = releaseAssets;
    inherit arch isLinux version;
  };

  # --- Build the Linux container from the tested Nix package.
  dockerImage = pkgs.callPackage ./docker.nix { inherit package; };

  # --- Cross-compile Windows on Linux, then smoke-test it on a Windows runner.
  # The GNU target avoids a dependency on the proprietary MSVC toolchain.
  releaseWindowsExecutable =
    let
      binary = pkgs.pkgsCross.mingwW64.callPackage ./binary.nix {
        inherit scalarJs;
        rustToolchain = toolchainFor "x86_64-pc-windows-gnu";
      };
    in
    pkgs.callPackage ./portable.nix {
      inherit binary version;
      arch = "x86_64";
      platform = "windows";
    };
in
{
  inherit releaseAssets releaseCheck;
}
# Containers run Linux binaries, so only Linux hosts expose this image output.
// lib.optionalAttrs isLinux { inherit dockerImage; }
# optionalAttrs leaves the Windows output absent on other hosts instead of
# exposing a value that those hosts cannot build.
// lib.optionalAttrs (system == "x86_64-linux") { inherit releaseWindowsExecutable; }
