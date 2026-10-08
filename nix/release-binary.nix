{
  lib,
  projectRoot ? ../.,
  makeRustPlatform,
  rustToolchain,
  scalarJs,
  stdenv,
}:
let
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "eliza-portable";
  version =
    (builtins.fromTOML (builtins.readFile (projectRoot + "/Cargo.toml"))).workspace.package.version;

  src = lib.fileset.toSource {
    root = projectRoot;
    fileset = lib.fileset.unions [
      (projectRoot + "/Cargo.toml")
      (projectRoot + "/Cargo.lock")
      (projectRoot + "/crates")
      (projectRoot + "/examples/rust-rig")
      (projectRoot + "/licences")
    ];
  };
  cargoLock.lockFile = projectRoot + "/Cargo.lock";
  cargoBuildFlags = [
    "--package"
    "eliza-cli"
    "--bin"
    "eliza"
  ];

  # The checked Nix package runs the suite. Cross-target executables are
  # exercised on their target OS by the release workflow instead.
  doCheck = false;
  RUSTFLAGS = "--remap-path-prefix=${rustToolchain}=/rust-toolchain";
  ELIZA_SCALAR_JS = scalarJs;
  postInstall = ''
    install -Dm444 ${projectRoot + "/LICENSE"} "$out/share/licenses/eliza/LICENSE"
    cp -R licences "$out/share/licenses/eliza/third-party"
  '';

  meta = {
    description = "1966 ELIZA through AI provider-compatible HTTP APIs";
    license = [
      lib.licenses.mit
      lib.licenses.asl20
    ];
    mainProgram = "eliza";
    platforms = [ stdenv.hostPlatform.system ];
  };
}
