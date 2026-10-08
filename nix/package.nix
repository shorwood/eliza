# `pkgs.callPackage` fills these dependencies from nixpkgs. The flake passes
# the pinned Rust toolchain and the systems that this repository supports.
{
  cacert,
  curl,
  lib,
  libclang,
  libxml2,
  makeRustPlatform,
  openssl,
  pkg-config,
  projectRoot ? ../.,
  rustToolchain,
  scalarJs,
  stdenv,
  supportedSystems,
}:
let
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "eliza";
  version =
    (builtins.fromTOML (builtins.readFile (projectRoot + "/Cargo.toml"))).workspace.package.version;

  # A narrow source set keeps unrelated files from invalidating the build.
  # Workspace crates and tests are included because `buildRustPackage` runs them.
  src = lib.fileset.toSource {
    root = projectRoot;
    fileset = lib.fileset.unions [
      (projectRoot + "/Cargo.toml")
      (projectRoot + "/Cargo.lock")
      (projectRoot + "/crates")
      (projectRoot + "/examples/rust-rig")
    ];
  };
  cargoLock.lockFile = projectRoot + "/Cargo.lock";
  cargoBuildFlags = [
    "--package"
    "eliza-cli"
    "--bin"
    "eliza"
  ];
  cargoTestFlags = [
    "--package"
    "eliza-cli"
  ];
  # Embedded Hurl is large enough to crash LLVM during release-profile tests.
  checkType = "debug";

  # Rust otherwise embeds its sysroot path in the executable, retaining the
  # entire toolchain at runtime. Remapping it keeps the package closure small;
  # the explicit prohibition makes a regression fail the build.
  RUSTFLAGS = "--remap-path-prefix=${rustToolchain}=/rust-toolchain";
  ELIZA_SCALAR_JS = scalarJs;
  LIBCLANG_PATH = "${libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = lib.optionalString stdenv.isLinux "-isystem ${stdenv.cc.libc.dev}/include";
  LD_LIBRARY_PATH = lib.makeLibraryPath [
    curl
    libxml2
    openssl
  ];
  disallowedReferences = [ rustToolchain ];

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [
    openssl
    libxml2
  ];

  # Embedded Hurl links libcurl when the package runs its tests.
  nativeCheckInputs = [
    cacert
    curl
  ];
  SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";

  # Keep the license notice in binary distributions. The container image uses
  # this package verbatim, so it receives the same notice automatically.
  postInstall = ''
    install -Dm444 ${projectRoot + "/LICENSE"} "$out/share/licenses/eliza/LICENSE"
    cp -R licences "$out/share/licenses/eliza/third-party"
  '';

  meta = {
    description = "Standalone classic ELIZA server with provider-compatible HTTP APIs";
    license = lib.licenses.mit;
    mainProgram = "eliza";
    platforms = supportedSystems;
  };
}
