fmt:
    cargo-fmt fmt --all
    find crates/cli/tests/http -type f -name '*.hurl' -exec hurlfmt --in-place {} +
    nixfmt flake.nix nix/*.nix

chat:
    sh examples/open-webui/run.sh

example-vercel-ai:
    pnpm --dir examples/vercel-ai install --frozen-lockfile
    pnpm --dir examples/vercel-ai run check
    cargo build --bin eliza --locked
    sh examples/with-eliza.sh pnpm --dir examples/vercel-ai start

example-rust-rig:
    cargo build --bin eliza --locked
    sh examples/with-eliza.sh cargo run -p example-rust-rig --locked

lint:
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    cargo-rlib rlib --workspace --all-targets --locked -- -D warnings

test:
    cargo test --workspace --all-targets --locked

test-http:
    cargo test -p eliza-cli --test http --locked

ok:
    cargo-fmt fmt --all -- --check
    find crates/cli/tests/http -type f -name '*.hurl' -exec hurlfmt --check {} +
    nixfmt --check flake.nix nix/*.nix
    deadnix --fail flake.nix nix
    statix check .
    cargo check --workspace --all-targets --locked
    cargo test --workspace --all-targets --locked
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    cargo-rlib rlib --workspace --all-targets --locked -- -D warnings
