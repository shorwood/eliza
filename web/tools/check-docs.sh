#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"

shellcheck web/tools/*.sh web/public/examples/smoke.sh
cargo-fmt fmt --manifest-path web/public/examples/rust-rig/Cargo.toml -- --check
cargo-clippy clippy --manifest-path web/public/examples/rust-rig/Cargo.toml --all-targets --locked -- -D warnings
cargo-rlib rlib --manifest-path web/public/examples/rust-rig/Cargo.toml --all-targets --locked -- -D warnings
pnpm --dir web/public/examples/vercel-ai install --frozen-lockfile
pnpm --dir web/public/examples/vercel-ai check
cargo build --bin eliza --locked

scratch=$(mktemp -d)
server_pid=''
cleanup() {
  if [ -n "$server_pid" ]; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf "$scratch"
}
trap cleanup EXIT
port=${ELIZA_DOCS_TEST_PORT:-18875}
api_root="http://127.0.0.1:$port"
"${CARGO_TARGET_DIR:-target}/debug/eliza" serve --bind "127.0.0.1:$port" \
  > "$scratch/server.log" 2>&1 &
server_pid=$!
for _ in {1..50}; do
  kill -0 "$server_pid"
  if curl --silent --fail --connect-timeout 1 --max-time 1 "$api_root/healthz" > /dev/null; then
    break
  fi
  sleep 0.1
done
export ELIZA_API_ROOT="$api_root"
unset ELIZA_API_KEY
bash web/public/examples/smoke.sh
pnpm --dir web/public/examples/vercel-ai start
cargo run --locked --manifest-path web/public/examples/rust-rig/Cargo.toml
node web/tools/export-docs.mjs "$api_root/openapi.json" https://example.com/api "$scratch/public"
node --input-type=module - "$scratch/public" <<'JS'
import assert from 'node:assert/strict';
import { readdir, readFile } from 'node:fs/promises';
const output = process.argv[2];
const schema = JSON.parse(await readFile(`${output}/openapi.json`, 'utf8'));
assert.equal(schema.servers[0].url, 'https://example.com/api');
assert.ok(schema.paths['/openai/v1/chat/completions']);
assert.ok(!Object.keys(schema.paths).some(path => path.startsWith('/api/')));
assert.deepEqual((await readdir(output)).sort(), ['LICENSE', 'README.md', 'docs', 'examples', 'llms.txt', 'openapi.json']);
for (const name of await readdir(`${output}/docs`)) {
  for (const [, target] of (await readFile(`${output}/docs/${name}`, 'utf8')).matchAll(/\]\(([^)]+)\)/g)) {
    if (!target.startsWith('http')) await readFile(`${output}/docs/${target}`);
  }
}
console.log('Public export paths and Markdown links passed');
JS
