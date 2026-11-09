# Local `nix develop` environment with the pinned Rust compiler, standard lint
# tools, and libraries used by ELIZA itself.
{
  curl,
  deadnix,
  git-cliff,
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
  rustToolchain,
  scalarJs,
  shellcheck,
  statix,
  stdenv,
}:
mkShell {
  # --- Match the environment expected by Cargo builds and native bindings.
  # Point Rust at the fetched Scalar bundle so local builds embed the same UI.
  ELIZA_SCALAR_JS = scalarJs;
  LIBCLANG_PATH = "${libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = lib.optionalString stdenv.isLinux "-isystem ${stdenv.cc.libc.dev}/include";

  # --- Supply the compiler and ELIZA's workflow tools directly.
  packages = [
    curl
    deadnix
    git-cliff
    hurl
    jq
    just
    libxml2
    nodejs_22
    nixfmt
    openssl
    pnpm
    pkg-config
    rustToolchain
    shellcheck
    statix
    stdenv.cc
  ];

  # --- Make native shared libraries available to locally run test binaries.
  LD_LIBRARY_PATH = lib.makeLibraryPath [
    curl
    libxml2
    openssl
    stdenv.cc.cc.lib
  ];
}
