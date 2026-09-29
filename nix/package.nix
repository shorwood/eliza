# `pkgs.callPackage` fills these dependencies from nixpkgs. The flake passes
# the pinned Rust toolchain and the systems that this repository supports.
{
  hurl,
  lib,
  makeRustPlatform,
  rustToolchain,
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
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  # A narrow source set keeps unrelated files from invalidating the build.
  # Tests and fixtures are included because `buildRustPackage` runs them.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../LICENSES
      ../NOTICE
      ../THIRD-PARTY-LICENSES.md
      ../examples/rust-rig
      ../fixtures
      ../src
      ../tests
    ];
  };
  cargoLock.lockFile = ../Cargo.lock;

  # Rust otherwise embeds its sysroot path in the executable, retaining the
  # entire toolchain at runtime. Remapping it keeps the package closure small;
  # the explicit prohibition makes a regression fail the build.
  RUSTFLAGS = "--remap-path-prefix=${rustToolchain}=/rust-toolchain";
  disallowedReferences = [ rustToolchain ];

  # The Rust integration tests invoke Hurl to verify the HTTP contracts.
  nativeCheckInputs = [ hurl ];

  # Keep the license notice in binary distributions. The container image uses
  # this package verbatim, so it receives the same notice automatically.
  postInstall = ''
    install -Dm444 ${../LICENSE} "$out/share/licenses/eliza/LICENSE"
    install -Dm444 ${../LICENSES/Apache-2.0.txt} \
      "$out/share/licenses/eliza/Apache-2.0.txt"
    install -Dm444 ${../NOTICE} "$out/share/licenses/eliza/NOTICE"
    install -Dm444 ${../THIRD-PARTY-LICENSES.md} \
      "$out/share/licenses/eliza/THIRD-PARTY-LICENSES.md"
  '';

  meta = {
    description = "Standalone classic ELIZA server with provider-compatible HTTP APIs";
    license = [
      lib.licenses.asl20
      lib.licenses.mit
    ];
    mainProgram = "eliza";
    platforms = supportedSystems;
  };
}
