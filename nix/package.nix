# The normal Nix package: build the server and run its Cargo checks.
# Portable release binaries use release/binary.nix instead.
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
  # --- Reuse the same Cargo source and embedded assets as release builds.
  common = import ./rust.nix {
    inherit
      lib
      projectRoot
      rustToolchain
      scalarJs
      ;
  };
  # makeRustPlatform wires the pinned Cargo and rustc into buildRustPackage.
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage (
  # `//` extends the shared attribute set with package-specific settings.
  common
  // {
    pname = "eliza";

    # --- Run the CLI tests in debug mode during the package build.
    cargoTestFlags = [
      "--package"
      "eliza-cli"
    ];
    # Embedded Hurl is large enough to crash LLVM during release-profile tests.
    checkType = "debug";
    # HTTP fixtures start a loopback server inside the Darwin build sandbox.
    __darwinAllowLocalNetworking = stdenv.buildPlatform.isDarwin;

    # --- Supply native libraries used by Rust dependencies and test fixtures.
    # Bindgen needs libclang; Linux also needs the libc headers at build time.
    LIBCLANG_PATH = "${libclang.lib}/lib";
    BINDGEN_EXTRA_CLANG_ARGS = lib.optionalString stdenv.isLinux "-isystem ${stdenv.cc.libc.dev}/include";
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      curl
      libxml2
      openssl
    ];
    # Catch any remaining reference that would retain the Rust toolchain in the
    # installed package's runtime closure.
    disallowedReferences = [ rustToolchain ];

    # pkg-config runs at build time; OpenSSL and libxml2 are linked dependencies.
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [
      openssl
      libxml2
    ];

    # Embedded Hurl links libcurl into test executables. Host libraries belong
    # in checkInputs so Darwin's linker receives their Nix library paths.
    checkInputs = [ curl ];
    # HTTPS fixtures need CA roots while the tests run.
    nativeCheckInputs = [ cacert ];
    SSL_CERT_FILE = "${cacert}/etc/ssl/certs/ca-bundle.crt";

    # --- Advertise only the systems with an exposed normal Nix package.
    meta = common.meta // {
      platforms = supportedSystems;
    };
  }
)
