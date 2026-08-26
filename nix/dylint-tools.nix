{
  cmake,
  dylintSrc,
  lib,
  openssl,
  pkg-config,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "dylint-tools";
  version = "6.0.1";
  src = dylintSrc;
  cargoLock.lockFile = "${dylintSrc}/Cargo.lock";
  cargoBuildFlags = [ "-p" "cargo-dylint" "-p" "dylint-link" ];
  nativeBuildInputs = [ cmake pkg-config ];
  buildInputs = [ openssl ];
  LD_LIBRARY_PATH = lib.makeLibraryPath [ openssl ];
  doCheck = false;
}
