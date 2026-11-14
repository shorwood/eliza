# Shared arguments for the tested package and portable release binaries.
# Both builds should use the same Cargo source and embedded API reference.
{
  lib,
  projectRoot,
  rustToolchain,
  scalarJs,
}:
{
  # --- Use one Cargo version, lockfile, and source tree for both builds.
  version =
    (builtins.fromTOML (builtins.readFile (projectRoot + "/Cargo.toml"))).workspace.package.version;
  # fileset selects only files Cargo needs, so changes elsewhere in the
  # checkout do not invalidate this build's source input and cache key.
  src = lib.fileset.toSource {
    root = projectRoot;
    fileset = lib.fileset.unions [
      (projectRoot + "/Cargo.toml")
      (projectRoot + "/Cargo.lock")
      (projectRoot + "/LICENSE")
      (projectRoot + "/licences")
      (projectRoot + "/crates")
      (projectRoot + "/examples/rust-rig")
    ];
  };
  cargoLock.lockFile = projectRoot + "/Cargo.lock";

  # --- Build the server executable rather than every workspace package.
  cargoBuildFlags = [
    "--package"
    "eliza-cli"
    "--bin"
    "eliza"
  ];

  # --- Keep the output portable and include the offline API reference.
  # Rust can embed its sysroot path in the executable. Remapping that path
  # prevents the compiler from becoming a runtime dependency.
  RUSTFLAGS = "--remap-path-prefix=${rustToolchain}=/rust-toolchain";
  # build.rs uses this hash-pinned file to embed Scalar into the server binary.
  ELIZA_SCALAR_JS = scalarJs;

  # Notices travel with normal packages, containers, and portable binaries.
  postInstall = ''
    mkdir -p "$out/share/doc/eliza"
    cp LICENSE "$out/share/doc/eliza/"
    cp -R licences "$out/share/doc/eliza/"
  '';

  # Nix consumers use this metadata for descriptions, licenses, and the CLI name.
  meta = {
    description = "1966 ELIZA through AI provider-compatible HTTP APIs";
    license = [
      lib.licenses.mit
      lib.licenses.asl20
    ];
    mainProgram = "eliza";
  };
}
