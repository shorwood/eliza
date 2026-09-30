# ELIZA Compatibility Server

A standalone, deterministic implementation of the classic ELIZA/DOCTOR
conversation engine, exposed through partial OpenAI, Anthropic, Gemini, and
Ollama HTTP contracts. It makes no upstream model calls and has no database,
Nano dependency, configuration file, or persistent session state.

The conversation engine executes a bounded MAD reconstruction with the
corrected 1966 Weizenbaum DOCTOR script, rather than a later BASIC-port
rewrite. This is a compatibility fixture and historical chatbot, not a medical
or therapeutic system.

## Install and run

### Nix flake

Install the executable from a checkout:

```sh
nix profile install .#eliza
eliza serve
```

Replace `.` with the published flake reference when installing remotely. The
same package can be run without adding it to a profile:

```sh
nix run .#eliza -- serve
```

### Docker image

The flake exposes `eliza:0.1.0` Linux image archives for x86-64 and ARM64.
On a matching Linux host, build and load the native image with:

```sh
nix build .#dockerImage
docker load --input result
docker run --rm -p 127.0.0.1:8787:8787 eliza:0.1.0
```

From a non-Linux host, select the Linux target explicitly and configure a
Linux Nix builder. Docker or Podman can load and run the result, but its Linux
VM does not automatically act as a Nix builder:

```sh
nix build .#packages.aarch64-linux.dockerImage
docker load --input result
```

The image runs as an unprivileged user and defaults to
`eliza serve --host 0.0.0.0`. Arguments after the image name replace that
default command, so include `serve` when supplying options:

```sh
docker run --rm -p 127.0.0.1:8787:8787 eliza:0.1.0 \
  serve --host 0.0.0.0 --log json
```

For a registry-published image, replace `eliza:0.1.0` with its full image
reference; the runtime arguments are unchanged.

The server listens on `http://127.0.0.1:8787` in both examples. Verify it with:

```sh
curl -s http://127.0.0.1:8787/healthz
```

Interactive Scalar documentation is served at `/docs`; the generated OpenAPI
3.1 document is served at `/openapi.json`.

## HTTP surface

Every provider route is namespaced. Unprefixed routes such as `/v1/models` and
`/v1/messages` are deliberately not mounted.

| Surface | Routes | Successful response transport |
| --- | --- | --- |
| System | `GET /healthz` | JSON |
| OpenAI | `GET /openai/v1/models`<br>`POST /openai/v1/chat/completions`<br>`POST /openai/v1/responses`<br>`POST /openai/v1/audio/speech`<br>`POST /openai/v1/embeddings` | JSON, binary audio, or SSE |
| Gemini OpenAI alias | `POST /gemini/v1beta/openai/chat/completions`<br>`POST /gemini/v1beta/openai/embeddings` | OpenAI JSON or SSE |
| Anthropic | `GET /anthropic/v1/models`<br>`POST /anthropic/v1/messages` | JSON; SSE when `stream: true` |
| Gemini | `GET /gemini/v1beta/models`<br>`POST /gemini/v1beta/models/{model}:generateContent`<br>`POST /gemini/v1beta/models/{model}:streamGenerateContent`<br>`POST /gemini/v1beta/models/fnv-embed:embedContent`<br>`POST /gemini/v1beta/models/fnv-embed:batchEmbedContents` | JSON; the stream action returns a JSON array, or SSE with `?alt=sse` |
| Ollama | `GET /ollama/api/tags`<br>`POST /ollama/api/chat`<br>`POST /ollama/api/embed` | NDJSON by default for chat streaming; otherwise JSON |
| Documentation | `GET /docs`<br>`GET /openapi.json` | HTML and JSON |

OpenAI and Gemini model lists advertise the model selected by `--model`, which
defaults to `eliza-1966`, plus the fixed `flite` speech model and
`fnv-embed` embedding model. Ollama advertises the configured chat
model and embedding model; Anthropic advertises only the chat model.

## Request examples

### OpenAI Chat Completions

```sh
curl -s http://127.0.0.1:8787/openai/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-1966","messages":[{"role":"user","content":"I am sad."}]}'
```

Gemini's OpenAI-compatible alias accepts the same request at
`/gemini/v1beta/openai/chat/completions`.

### OpenAI Responses

```sh
curl -s http://127.0.0.1:8787/openai/v1/responses \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-1966","input":"I need help."}'
```

### OpenAI Speech

```sh
curl -s http://127.0.0.1:8787/openai/v1/audio/speech \
  -H 'Content-Type: application/json' \
  -d '{"model":"flite","input":"Hello from the nineties.","voice":"Kore","response_format":"wav"}' \
  --output speech.wav
```

The endpoint supports MP3, WAV, and signed 16-bit little-endian PCM. Set
`"stream_format":"sse"` for base64 `speech.audio.delta` events followed by a
`speech.audio.done` event.

### Embeddings

```sh
curl -s http://127.0.0.1:8787/openai/v1/embeddings \
  -H 'Content-Type: application/json' \
  -d '{"model":"fnv-embed","input":["Hello world","Hello there"],"dimensions":256}'
```

The Gemini OpenAI alias accepts the same request at
`/gemini/v1beta/openai/embeddings`. Native Gemini clients can use
`models/fnv-embed:embedContent` or `:batchEmbedContents`; Ollama
clients can use `/ollama/api/embed`. All surfaces use 256 dimensions by
default and accept values from 1 through 1024.

### Anthropic Messages

```sh
curl -s http://127.0.0.1:8787/anthropic/v1/messages \
  -H 'Content-Type: application/json' \
  -H 'anthropic-version: 2023-06-01' \
  -d '{"model":"eliza-1966","max_tokens":128,"messages":[{"role":"user","content":"I am sad."}]}'
```

### Gemini Generate Content

```sh
curl -s http://127.0.0.1:8787/gemini/v1beta/models/eliza-1966:generateContent \
  -H 'Content-Type: application/json' \
  -d '{"contents":[{"role":"user","parts":[{"text":"I am sad."}]}]}'
```

Gemini speech uses the same route with the dedicated model and audio modality:

```sh
curl -s http://127.0.0.1:8787/gemini/v1beta/models/flite:generateContent \
  -H 'Content-Type: application/json' \
  -d '{"contents":[{"role":"user","parts":[{"text":"Hello from the nineties."}]}],"generationConfig":{"responseModalities":["AUDIO"],"speechConfig":{"voiceConfig":{"voice":"retro"}},"responseFormat":{"audio":{"mimeType":"AUDIO_WAV"}}}}'
```

Gemini speech supports `AUDIO_WAV`, `AUDIO_L16`, `AUDIO_MULAW`, and
`AUDIO_ALAW` at 8, 16, or 24 kHz. Its streaming action defaults to L16 and
retains the existing JSON-array or `?alt=sse` transports. Select both under
`generationConfig.responseFormat.audio` using `mimeType` and `sampleRate`.

### Ollama Chat

```sh
curl -s http://127.0.0.1:8787/ollama/api/chat \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-1966","stream":false,"messages":[{"role":"user","content":"I am sad."}]}'
```

## Runtime configuration

`eliza serve` is the complete application configuration surface:

| Option | Default | Contract |
| --- | --- | --- |
| `--host` | `127.0.0.1` | Listener IP address |
| `--port` | `8787` | Listener port |
| `--model` | `eliza-1966` | Nonblank model advertised by catalog routes |
| `--auth` | `none` | `none` or `bearer` |
| `--bearer-token` | unset | Required and nonempty when `--auth bearer` is selected |
| `--cors` | `none` | `none` or `permissive` |
| `--stream-delay-ms` | `0` | Delay inserted between emitted stream chunks |
| `--max-input-chars` | `8000` | Maximum combined characters in instructions, history, and tool definitions |
| `--max-history-messages` | `200` | Maximum normalized transcript entries accepted in one request |
| `--log` | `text` | `text` or `json` tracing output |

Both limits must be greater than zero. `RUST_LOG` may be used as the standard
tracing filter; it defaults to `info` when absent or invalid.

Inspect the authoritative CLI contract with:

```sh
eliza serve --help
```

For the container image, use
`docker run --rm eliza:0.1.0 serve --help`.

### Authentication

`--auth bearer --bearer-token <token>` protects every provider route with one
shared secret, rendered through each provider's native authentication scheme:

| Surface | Required header |
| --- | --- |
| OpenAI, Gemini OpenAI alias, Ollama | `Authorization: Bearer <token>` |
| Anthropic | `x-api-key: <token>` |
| Native Gemini | `x-goog-api-key: <token>` |

Health and documentation routes remain public. Authentication failures and
request failures use provider-native JSON envelopes and include a stable
`x-eliza-error-code` response header.

## Compatibility boundaries

- Request and response envelopes are typed per provider; unknown object fields
  are tolerated, while unsupported known variants are rejected explicitly.
- Conversation input is text-only. Multimodal content, OpenAI structured
  output, and Ollama structured or reasoning output are not implemented.
- Speech is intentionally retro, deterministic, and English/ASCII-oriented.
  It uses Flite's bundled 8 kHz diphone voice; unsupported Unicode is rejected
  and output is capped at 120 seconds.
- Embeddings are deterministic, local feature hashes rather than learned model
  output. They make no upstream calls, support only floating-point vectors,
  and are intended for compatibility tests and lightweight similarity checks.
- Arbitrary voice names select stable pitch profiles. Style prose only
  recognizes `whisper`, `calm`/`soft`, `excited`/`cheerful`, `high`,
  `low`/`deep`, `fast`/`quick`, and `slow`; other words are ignored.
- A request replays its user history through a fresh ELIZA session. State is
  not retained between HTTP requests.
- Usage counts are deterministic approximations, not provider tokenizer output.
- Text streaming splits an already-computed deterministic response into native
  SSE or NDJSON records. Speech streaming synthesizes and encodes incrementally
  through a bounded two-worker pool; `--stream-delay-ms` paces delivery.
- Input and history limits apply after provider contracts are lowered into the
  shared conversation contract.

## Deterministic tool fixture

Every generation surface accepts client-defined function tools. Tool
definitions do not affect an ordinary ELIZA prompt. To request a call, make the
complete latest user message:

```text
@tool <offered-name> <json-object>
```

For example:

```text
@tool echo {"value":"hello"}
```

The named function must be present in the request and permitted by the
provider's tool-choice policy. ELIZA returns one call in the provider's native
shape. A following request that preserves the directive and provider-native
call, then appends its tool result, receives `TOOL CALL COMPLETE`.

The fixture is stateless, supports one call at a time, passes the JSON object
through unchanged, and does not validate arguments against the supplied JSON
Schema.

## Client examples

The Docker Compose example runs Open WebUI preconfigured for ELIZA chat and
speech. With a Docker daemon running, this command builds the server and starts
the interface at `http://127.0.0.1:3000` or `http://localhost:3000`:

```sh
nix develop --command just chat
```

Choose `eliza-1966`, send a message, then use the speaker button beneath its
reply to exercise `flite`. To test a tool round trip, enable
`Hello MCP` under **+ > Integrations > Tools**, then send:

```text
@tool hello_greet {"language":"french","name":"Alice"}
```

Open WebUI namespaces the MCP server's `greet` tool as `hello_greet`, calls it,
and ELIZA answers `TOOL CALL COMPLETE` after receiving its result. Stop the
stack with Ctrl-C. Open WebUI stores its state in the
`eliza-open-webui-data` Docker volume; set `ELIZA_OPEN_WEBUI_PORT` to change
the host port. All three services share a private Compose network; only the
WebUI is exposed to the host. This example raises ELIZA's input limit to
65,536 characters to accommodate Open WebUI's request metadata and paces
streamed chunks at 50 ms so incremental rendering remains visible. Non-Linux
hosts need the Linux Nix builder described under Docker image installation
above.

Real Vercel AI SDK and Rig examples cover unary, streaming, model-list, and tool
round-trip compatibility where their provider clients expose those operations:

```sh
nix develop --command just example-vercel-ai
nix develop --command just example-rust-rig
```

See [examples/README.md](examples/README.md) for their exact coverage. Set
`ELIZA_EXAMPLE_PORT` to use a port other than `8787`.

## Development

Enter the pinned shell and run the complete quality gate:

```sh
nix develop
just ok
```

The gate checks formatting, the one-request-per-Hurl-file contract, compilation,
unit tests, CLI UI snapshots, HTTP contracts, Clippy, and rlib Dylint rules.
Use `just test-http` for only the Hurl-backed HTTP suites.

## License

ELIZA is available under the [MIT License](LICENSE).
