{ fenix, system }:

# Rust, the editor, and Dylint share one pinned nightly compiler.
let
  targetTriple = {
    x86_64-linux = "x86_64-unknown-linux-gnu";
    aarch64-linux = "aarch64-unknown-linux-gnu";
    x86_64-darwin = "x86_64-apple-darwin";
    aarch64-darwin = "aarch64-apple-darwin";
  }.${system} or (throw "Unsupported system: ${system}");
  datedToolchain = fenix.packages.${system}.toolchainOf {
    channel = "nightly";
    date = "2026-04-16";
    sha256 = "1kbgfwz1hd2z6fkq97650in4fwa13y9n6rhjankh5hbmis8mqkn3";
  };
in {
  toolchain = datedToolchain.withComponents [
    "cargo"
    "clippy"
    "rustfmt"
    "rust-analyzer"
    "rust-src"
    "llvm-tools-preview"
    "rustc"
    "rustc-dev"
  ];
  inherit targetTriple;
  toolchainLabel = "nightly-${targetTriple}";
}
