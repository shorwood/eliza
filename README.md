# ELIZA

A small, deterministic compatibility server for OpenAI, Anthropic, Gemini, and
Ollama clients, powered by the 1966 ELIZA/DOCTOR script.

Use it as a local fixture for client development, demos, and contract tests.
Responses are repeatable, and the server needs no model downloads, GPU,
credentials, database, configuration file, or upstream network calls.

ELIZA implements a focused subset of each provider API. It is a historical
chatbot and compatibility fixture, not a general-purpose model or therapeutic
system.

## Quick start

Run the published Docker image:

```sh
docker run --rm -p 127.0.0.1:8787:8787 ghcr.io/shorwood/eliza:latest
```

Or run the flake directly without Docker:

```sh
nix run github:shorwood/eliza/v1.0.0 -- serve
```

See the [self-hosting guide](https://eliza.nwrx.io/docs/self-hosting) for a
Testcontainers test, image pinning, and local versus hosted API URLs.
Then send an OpenAI Chat Completions request:

```sh
curl -sS http://127.0.0.1:8787/openai/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-1966","messages":[{"role":"user","content":"I am sad."}]}'
```

The server also exposes:

- health checks at `GET /healthz`;
- interactive API documentation at `/docs`; and
- its OpenAPI 3.1 document at `/openapi.json`.

## API compatibility

All provider routes are namespaced; unprefixed routes such as `/v1/models` are
not mounted.

| Provider | Prefix | Supported APIs |
| --- | --- | --- |
| OpenAI | `/openai/v1` | Models, Chat Completions, Responses, speech, embeddings, and image generation |
| Anthropic | `/anthropic/v1` | Models and Messages |
| Gemini | `/gemini/v1beta` | Models, text, image and speech generation, streaming, and embeddings |
| Gemini OpenAI alias | `/gemini/v1beta/openai` | Chat Completions and embeddings |
| Ollama | `/ollama/api` | Tags, chat, and embeddings |

Requests and responses use provider-native envelopes. Streaming uses SSE,
NDJSON, or Gemini's JSON-array transport as appropriate. See `/docs` for the
exact routes and schemas.

### Built-in models

| Model | Capability | Behavior |
| --- | --- | --- |
| `eliza-1966` | Chat | A bounded MAD reconstruction using the corrected 1966 DOCTOR script |
| `flite` | Speech | Local retro speech in MP3, WAV, or PCM |
| `fnv-embed` | Embeddings | Local feature hashing with 1–1024 dimensions |
| `eliza-retro-image` | Image generation | Prompt-seeded cellular automata rendered as PNG |

Model names can be customized independently for each local engine:

```sh
eliza serve \
  --model-chat chat-gpt-4 \
  --model-chat chat-gpt-5 \
  --model-embeddings text-embedding-3-large
```

Repeat a model option to advertise several names. Supplied names replace that
modality's defaults; duplicates keep their first occurrence. These names still
use the local engines listed above. `--chat-model` remains an alias for
`--model-chat`. Model arguments accept literal names.

Chat keeps accepting arbitrary request model IDs, with configured names controlling
its catalogs. Embedding, speech, and image-generation routes accept their configured
aliases. Gemini embedding bodies must name the same model resource as their URL,
including every item in a batch. Use distinct names across modalities for clear
Gemini generation routing; catalogs show a shared name once, with the first
modality's metadata.

Chat endpoints also accept bounded PNG and JPEG input and return deterministic
pixel statistics. They do not perform OCR, object recognition, or face
detection.

Text-generation endpoints support streaming, structured JSON output, and one
deterministic function call at a time. To request a call, make the latest user
message:

```text
@tool <offered-name> <json-object>
```

For example, `@tool echo {"value":"hello"}` calls an offered `echo` function.
A follow-up preserving that directive and call, then appending the
provider-native tool result, receives `TOOL CALL COMPLETE`.

### Reasoning traces

Native reasoning controls can expose the deterministic rule-selection trace
that produced an answer:

| API | Opt-in |
| --- | --- |
| OpenAI Responses | `"reasoning":{"summary":"auto"}` |
| Anthropic Messages | `"thinking":{"type":"enabled","budget_tokens":1024}` with `max_tokens` greater than the budget |
| Gemini Generate Content | `"thinkingConfig":{"includeThoughts":true}` under `generationConfig` |
| Ollama Chat | `"think":true` or a named level such as `"high"` |

The trace reports normalized input, ranked keywords, the final zero-based
rule coordinates, and whether the answer came from a keyword, memory, `NONE`,
or a tool. It is mechanical diagnostics, not chain of thought. Effort levels
and budgets are validated compatibility controls and do not change ELIZA's
algorithm. OpenAI Chat Completions and the Gemini OpenAI alias intentionally
return no reasoning extension.

## Runtime options

Run `eliza serve --help` for the complete CLI contract. The main options are:

| Option | Default | Purpose |
| --- | --- | --- |
| `--config` | unset | TOML file for server options; explicit CLI flags take precedence |
| `--bind` | `127.0.0.1:8787` | Listener address |
| `--runtime-workers` | Tokio automatic sizing | Async runtime thread count; explicit flag overrides `TOKIO_WORKER_THREADS` |
| `--speech-workers` | `2` | Shared limit on simultaneous speech jobs across providers |
| `--model-chat` | `eliza-1966` | Repeatable chat names; also accepts `--chat-model` |
| `--model-embeddings` | `fnv-embed` | Repeatable embedding names |
| `--model-images` | `eliza-retro-image` | Repeatable image-generation names |
| `--model-speech` | `flite` | Repeatable speech names |
| `--api-key` | unset | Enable provider-native authentication |
| `--allow-any-origin` | off | Enable permissive CORS |
| `--max-input-chars` | `8000` | Bound combined request text |
| `--max-history-messages` | `200` | Bound chat history |
| `--stream-delay-ms` | `0` | Pace streamed chunks |
| `--json-logs` | off | Emit JSON logs |

The TOML file uses snake_case keys. Most keys match the CLI flags with hyphens
replaced by underscores. Model flags use `chat_models`, `embedding_models`,
`image_models`, and `speech_models`; the two switch keys are
`should_allow_any_origin` and `should_use_json_logs`. Model names are arrays;
each model array supplied on the CLI replaces its file value:

```toml
bind = "127.0.0.1:8787"
speech_workers = 4
chat_models = ["eliza-1966", "local-chat"]
should_allow_any_origin = true
```

Run `eliza serve --config eliza.toml`. Omitted keys use the CLI defaults.
For hosted admission, put the policy under `[hosted]` in the same TOML file.
Secrets still come from `ELIZA_INGRESS_SECRET` and `ELIZA_ENTITLEMENT_SECRET`.

Container arguments replace its default command. Start custom arguments with
`serve --bind 0.0.0.0:8787` so the mapped port remains reachable.

When `--api-key` is set, use a bearer token for OpenAI, the Gemini OpenAI
alias, and Ollama; `x-api-key` for Anthropic; and `x-goog-api-key` for native
Gemini. Health and documentation routes remain public. `RUST_LOG` controls the
tracing filter.

## Scope

- Each request starts a fresh ELIZA session and replays the supplied user
  history. Nothing persists between requests.
- Known unsupported provider variants are rejected with provider-native error
  envelopes. Unknown object fields are tolerated.
- Returned reasoning signatures are stable, non-cryptographic fixture tokens;
  clients may preserve them in conversation history.
- Image analysis is a coarse local measurement; image generation is cellular
  automata; embeddings are feature hashes; and speech uses Flite's bundled
  voice. None are learned models.
- Remote image URLs and provider file references are never fetched.
- Inputs, decoded media, schemas, generated output, and streaming work are
  bounded. This server is designed for development fixtures, not production AI
  inference.

## Client examples

An Open WebUI stack and working Vercel AI SDK and Rig clients live under
[`examples/`](examples/README.md):

```sh
nix develop -c just chat
nix develop -c just example-vercel-ai
nix develop -c just example-rust-rig
```

The commands build and start ELIZA for the example, then stop it when finished.
The Open WebUI example requires a running Docker daemon.

## Development

Enter the pinned development shell and run the complete quality gate:

```sh
nix develop
just ok
```

The gate checks formatting, Nix, compilation, tests, HTTP contracts, Clippy,
and Dylint.

## License

ELIZA is available under the [MIT License](LICENSE).
The API reference and speech data retain their own terms; see the
[third-party licences](licences).
