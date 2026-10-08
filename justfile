fmt:
    cargo-fmt fmt --all
    hurlfmt --in-place crates/cli/tests/http/*.hurl crates/cli/tests/http/*/*.hurl
    nixfmt flake.nix nix/*.nix

bench-generation *args:
    cargo bench -p eliza-benches --bench generation --bench completion --bench speech --locked -- {{args}}

bench-e2e *args:
    cargo build --release -p eliza-cli --bin eliza --locked
    cargo build --release -p eliza-benches --bin eliza-bench --locked
    target/release/eliza-bench {{args}}

bench-smoke:
    just bench-generation --test
    just bench-e2e --smoke

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
    hurlfmt --check crates/cli/tests/http/*.hurl crates/cli/tests/http/*/*.hurl
    nixfmt --check flake.nix nix/*.nix
    deadnix --fail flake.nix nix
    statix check .
    cargo check --workspace --all-targets --locked
    cargo test --workspace --all-targets --locked
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    cargo-rlib rlib --workspace --all-targets --locked -- -D warnings
