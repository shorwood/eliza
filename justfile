fmt:
    cargo fmt --all

chat:
    AICHAT_CONFIG_FILE="$PWD/fixtures/aichat.yaml" aichat

example-vercel-ai:
    pnpm --dir examples/vercel-ai install --frozen-lockfile
    pnpm --dir examples/vercel-ai run check
    cargo build --bin eliza --locked
    sh examples/with-eliza.sh pnpm --dir examples/vercel-ai start

example-rust-rig:
    cargo build --bin eliza --locked
    sh examples/with-eliza.sh cargo run -p example-rust-rig --locked

rlib-library:
    cargo build --manifest-path ../rlib/Cargo.toml -p rlib-lint --no-default-features --features bon,derive_more,miette,serde,strum,thiserror --locked
    host=$(rustc --version --verbose | sed -n 's/^host: //p'); case "$host" in *darwin*) extension=dylib ;; *) extension=so ;; esac; source="../rlib/target/debug/librlib_lint.$extension"; target="../rlib/target/debug/librlib_lint@nightly-$host.$extension"; test -f "$source"; cp "$source" "$target"

lint: rlib-library
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    host=$(rustc --version --verbose | sed -n 's/^host: //p'); case "$host" in *darwin*) extension=dylib ;; *) extension=so ;; esac; plugin="$PWD/../rlib/target/debug/librlib_lint@nightly-$host.$extension"; plugin_hash=$(cksum "$plugin" | awk '{print $1}'); CARGO_TARGET_DIR="$PWD/target/dylint/$plugin_hash" CARGO_BUILD_JOBS=1 DYLINT_LIBRARY_PATH="$PWD/../rlib/target/debug" DYLINT_RUSTFLAGS="-Dwarnings" cargo dylint --no-deps --lib rlib_lint --workspace -- --all-targets --locked

test:
    cargo test --workspace --all-targets --locked

ok: rlib-library
    cargo fmt --all --check
    cargo check --workspace --all-targets --locked
    cargo test --workspace --all-targets --locked
    cargo-clippy clippy --workspace --all-targets --locked -- -D warnings
    host=$(rustc --version --verbose | sed -n 's/^host: //p'); case "$host" in *darwin*) extension=dylib ;; *) extension=so ;; esac; plugin="$PWD/../rlib/target/debug/librlib_lint@nightly-$host.$extension"; plugin_hash=$(cksum "$plugin" | awk '{print $1}'); CARGO_TARGET_DIR="$PWD/target/dylint/$plugin_hash" CARGO_BUILD_JOBS=1 DYLINT_LIBRARY_PATH="$PWD/../rlib/target/debug" DYLINT_RUSTFLAGS="-Dwarnings" cargo dylint --no-deps --lib rlib_lint --workspace -- --all-targets --locked
