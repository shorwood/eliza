# --- Keep handwritten sources and fixtures in canonical format.
# Format Rust, Hurl HTTP fixtures, and Nix files in every subdirectory.
fmt:
    cargo-fmt fmt --all
    find crates/cli/tests/http -type f -name '*.hurl' -exec hurlfmt --in-place {} +
    nixfmt flake.nix
    find nix -type f -name '*.nix' -exec nixfmt {} +

# --- Exercise provider clients through the running server.
# Build ELIZA once, then run Vercel AI SDK and Rig against fresh local servers.
smoke:
    pnpm --dir examples/vercel-ai install --frozen-lockfile
    pnpm --dir examples/vercel-ai run check
    cargo build --bin eliza --locked
    sh examples/with-eliza.sh pnpm --dir examples/vercel-ai start
    sh examples/with-eliza.sh cargo run -p example-rust-rig --locked

# --- Reject static issues before running tests.
# Check Rust, Hurl, and Nix formatting plus Clippy without edits.
lint:
    cargo-fmt fmt --all -- --check
    find crates/cli/tests/http -type f -name '*.hurl' -exec hurlfmt --check {} +
    nixfmt --check flake.nix
    find nix -type f -name '*.nix' -exec nixfmt --check {} +
    deadnix --fail flake.nix nix
    statix check .
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings

# --- Prove behavior across the Rust workspace.
# Run the workspace tests, including CLI and HTTP contract fixtures.
test:
    cargo test --workspace --all-targets --locked

# --- Use one gate for local work and CI.
# Run lint and tests, the same gate used by CI.
ok: lint test
