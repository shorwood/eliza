# Build the portable executable shared by the release packaging recipes.
# Unlike package.nix, this derivation is configured for each release target.
{
  lib,
  projectRoot ? ../..,
  makeRustPlatform,
  rustToolchain,
  scalarJs,
  stdenv,
}:
let
  # --- Reuse the same Cargo source and embedded Scalar bundle.
  common = import ../rust.nix {
    inherit
      lib
      projectRoot
      rustToolchain
      scalarJs
      ;
  };
  # The caller supplies Cargo/rustc plus the target standard library.
  rustPlatform = makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
# --- Produce a binary for the requested target; test the artifact separately.
rustPlatform.buildRustPackage (
  common
  // {
    pname = "eliza-portable";
    # Cross-built targets cannot run Cargo tests on the build host. Release CI
    # runs the Linux/macOS artifacts locally and Windows on a Windows runner.
    doCheck = false;
    meta = common.meta // {
      platforms = [ stdenv.hostPlatform.system ];
    };
  }
)
