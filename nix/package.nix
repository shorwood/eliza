# `pkgs.callPackage` fills these dependencies from nixpkgs. The flake passes
# the pinned Rust toolchain and the systems that this repository supports.
{
  hurl,
  lib,
  makeRustPlatform,
  projectRoot ? ../.,
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
  version =
    (builtins.fromTOML (builtins.readFile (projectRoot + "/crates/cli/Cargo.toml"))).package.version;

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
    install -Dm444 ${projectRoot + "/LICENSE"} "$out/share/licenses/eliza/LICENSE"
  '';

  meta = {
    description = "Standalone classic ELIZA server with provider-compatible HTTP APIs";
    license = lib.licenses.mit;
    mainProgram = "eliza";
    platforms = supportedSystems;
  };
}
