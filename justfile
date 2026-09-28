fmt:
    cargo-fmt fmt --all
    hurlfmt --in-place tests/http/public/*.hurl tests/http/auth/*.hurl

chat:
    cargo build --bin eliza --locked
    ELIZA_EXAMPLE_PORT=8787 \
    AICHAT_CONFIG_FILE="$PWD/fixtures/aichat.yaml" \
    sh examples/with-eliza.sh aichat

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

check-hurl-layout:
    @for file in tests/http/public/*.hurl tests/http/auth/*.hurl; do request_count=$(rg -c '^(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS) ' "$file"); test "$request_count" -eq 1 || { echo "$file: expected one HTTP request, found $request_count" >&2; exit 1; }; done

test:
    cargo test --workspace --all-targets --locked

test-http: check-hurl-layout
    cargo test --test http --locked

ok: check-hurl-layout
    cargo-fmt fmt --all -- --check
    hurlfmt --check tests/http/public/*.hurl tests/http/auth/*.hurl
    cargo check --workspace --all-targets --locked
    cargo test --workspace --all-targets --locked
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    cargo-rlib rlib --workspace --all-targets --locked -- -D warnings
