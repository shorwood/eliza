# ELIZA Compatibility Server

Standalone classic ELIZA/DOCTOR HTTP server for local provider-compatibility
tests. It does not depend on Nano crates, Nano config, Nano auth, or a database.

The bundled script follows the 1966 Weizenbaum DOCTOR script lineage from
ELIZAGEN, not a BASIC-port rewrite.

## Run

```sh
nix develop -c cargo run -- serve --port 8787
```

The development shell includes AIChat, an interactive terminal client configured
for this server. With the server running, start it in another terminal:

```sh
nix develop -c just chat
```

Runnable Vercel AI and Rig compatibility examples live in
[examples](examples/README.md).

All provider-compatible routes are always available; choose the path that
matches the client you are testing. Runtime configuration is CLI-only; inspect
the current contract with:

```sh
nix develop -c cargo run -- serve --help
```

Interactive docs are mounted at `http://127.0.0.1:8787/docs`, with raw OpenAPI
at `http://127.0.0.1:8787/openapi.json`.

## OpenAI Chat Completions

```sh
curl -s http://127.0.0.1:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-doctor","messages":[{"role":"user","content":"I am sad."}]}'
```

The Gemini OpenAI-compatible alias is also available:

```text
http://127.0.0.1:8787/v1beta/openai/chat/completions
```

## Anthropic Messages

```sh
curl -s http://127.0.0.1:8787/v1/messages \
  -H 'Content-Type: application/json' \
  -H 'anthropic-version: 2023-06-01' \
  -d '{"model":"eliza-doctor","max_tokens":128,"messages":[{"role":"user","content":"I am sad."}]}'
```

## Gemini Generate Content

```sh
curl -s http://127.0.0.1:8787/v1beta/models/eliza-doctor:generateContent \
  -H 'Content-Type: application/json' \
  -d '{"contents":[{"role":"user","parts":[{"text":"I am sad."}]}]}'
```

Streaming surfaces return provider-shaped SSE events where provider SDKs expect
them, but the engine still computes one deterministic ELIZA response and splits
it into chunks. This is a historical chatbot adapter, not a medical or
therapeutic system.
