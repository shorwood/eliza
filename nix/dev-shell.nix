# rlib supplies the pinned Rust compiler and lint tooling. This shell adds only
# the tools and native libraries used directly by ELIZA's development workflow.
{
  curl,
  deadnix,
  docker-client,
  docker-compose,
  hurl,
  jq,
  just,
  lib,
  mkShell,
  nodejs_22,
  nixfmt,
  openssl,
  pkg-config,
  pnpm,
  rlibShell,
  shellcheck,
  statix,
  stdenv,
}:
mkShell {
  inputsFrom = [ rlibShell ];
  packages = [
    curl
    deadnix
    docker-client
    docker-compose
    hurl
    jq
    just
    nodejs_22
    nixfmt
    openssl
    pnpm
    pkg-config
    shellcheck
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
