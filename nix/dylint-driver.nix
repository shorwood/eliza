{
  cmake,
  dylintSrc,
  lib,
  makeWrapper,
  openssl,
  pkg-config,
  rustPlatform,
  rustToolchain,
  toolchainLabel,
}:

rustPlatform.buildRustPackage {
  pname = "dylint-driver";
  version = "6.0.1";
  src = dylintSrc;
  postUnpack = ''
    sourceRoot="$sourceRoot/driver"
  '';
  cargoLock.lockFile = "${dylintSrc}/driver/Cargo.lock";
  RUSTUP_TOOLCHAIN = toolchainLabel;
  postPatch = ''
    cat >> Cargo.toml <<'EOF'

[[bin]]
name = "dylint-driver"
path = "src/main.rs"
EOF

    cat > src/main.rs <<'EOF'
#![feature(rustc_private)]

fn main() -> anyhow::Result<()> {
    let args = std::env::args_os().collect::<Vec<_>>();
    dylint_driver::dylint_driver(&args)
}
EOF
  '';
  cargoBuildFlags = [ "--bin" "dylint-driver" ];
  nativeBuildInputs = [ cmake makeWrapper pkg-config ];
  buildInputs = [ openssl ];
  LD_LIBRARY_PATH = lib.makeLibraryPath [ openssl ];
  doCheck = false;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/${toolchainLabel}"
    find ../target/nightly -type f -path '*/release/dylint-driver' \
      -exec install -m755 {} "$out/${toolchainLabel}/.dylint-driver-unwrapped" \;
    makeWrapper "$out/${toolchainLabel}/.dylint-driver-unwrapped" \
      "$out/${toolchainLabel}/dylint-driver" \
      --prefix LD_LIBRARY_PATH : "${rustToolchain}/lib" \
      --prefix DYLD_LIBRARY_PATH : "${rustToolchain}/lib"
    test -x "$out/${toolchainLabel}/dylint-driver"
    runHook postInstall
  '';
}
