# rlib supplies the pinned Rust compiler and lint tooling. This shell adds only
# the tools and native libraries used directly by ELIZA's development workflow.
{
  aichat,
  curl,
  deadnix,
  hurl,
  just,
  lib,
  mkShell,
  nodejs_22,
  nixfmt,
  openssl,
  pkg-config,
  pnpm,
  rlibShell,
  statix,
  stdenv,
}:
mkShell {
  inputsFrom = [ rlibShell ];
  packages = [
    aichat
    curl
    deadnix
    hurl
    just
    nodejs_22
    nixfmt
    openssl
    pnpm
    pkg-config
    statix
    stdenv.cc
  ];

  # Native Rust dependencies need OpenSSL and the C++ runtime at execution
  # time as well as during compilation.
  LD_LIBRARY_PATH = lib.makeLibraryPath [
    openssl
    stdenv.cc.cc.lib
  ];
}
