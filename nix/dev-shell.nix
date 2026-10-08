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
  libclang,
  libxml2,
  mkShell,
  nodejs_22,
  nixfmt,
  openssl,
  pkg-config,
  pnpm,
  rlibShell,
  scalarJs,
  shellcheck,
  statix,
  stdenv,
}:
mkShell {
  ELIZA_SCALAR_JS = scalarJs;
  LIBCLANG_PATH = "${libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = lib.optionalString stdenv.isLinux "-isystem ${stdenv.cc.libc.dev}/include";
  inputsFrom = [ rlibShell ];
  packages = [
    curl
    deadnix
    docker-client
    docker-compose
    hurl
    jq
    just
    libxml2
    nodejs_22
    nixfmt
    openssl
    pnpm
    pkg-config
    shellcheck
    statix
    stdenv.cc
  ];

  # Native Rust dependencies also need their shared libraries during tests.
  LD_LIBRARY_PATH = lib.makeLibraryPath [
    curl
    libxml2
    openssl
    stdenv.cc.cc.lib
  ];
}
