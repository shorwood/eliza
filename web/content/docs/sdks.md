# Standalone examples

The exported `examples/` directory works without the private source repository.
Download [the smoke script](../examples/smoke.sh),
[the TypeScript entrypoint](../examples/vercel-ai/main.ts), or
[the Rig entrypoint](../examples/rust-rig/src/main.rs) with their project/lock files.
Each example reads `ELIZA_API_ROOT` and optional `ELIZA_API_KEY`. Use the root
before the provider prefix, such as `http://127.0.0.1:8787` for a local instance.
Strip a trailing slash. Hosted service access is pending deployment.

## curl

Requires Bash, curl, and jq. The script checks a completion, a complete SSE stream,
and a provider-native validation error. Its retries are appropriate for this
stateless fixture; do not copy POST retry policy into arbitrary production APIs.

```sh
ELIZA_API_ROOT=http://127.0.0.1:8787 bash examples/smoke.sh
```

## Vercel AI SDK

Requires Node.js 22.18+ and pnpm 10. Versions are pinned with a lockfile. Calls
explicitly select the OpenAI provider, so they do not use Vercel AI Gateway.
The example verifies completion/stream equality, handles stream errors, and uses
a 30-second abort deadline with SDK retries disabled.

```sh
cd examples/vercel-ai
pnpm install --frozen-lockfile
pnpm check
ELIZA_API_ROOT=http://127.0.0.1:8787 pnpm start
```

## Rust Rig

Requires Rust 1.94+ with edition 2024. This is an independent Cargo
project, not a path dependency on the private implementation. The example uses
Rig's OpenAI Completions client and verifies completion/stream equality under
a 30-second overall deadline. Run its pinned dependencies with:

```sh
ELIZA_API_ROOT=http://127.0.0.1:8787 \
  cargo run --locked --manifest-path examples/rust-rig/Cargo.toml
```

These are minimal OpenAI Chat Completions examples. The [provider table](providers.md)
lists other supported routes; SDK-specific URL suffix handling can differ, so do
not blindly reuse one SDK's base URL for another provider.
