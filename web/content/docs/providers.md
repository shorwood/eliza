# Provider routes and capabilities

Append the provider prefix to `ELIZA_API_ROOT`. Hosted `/api` belongs in the root;
do not append it again. Unprefixed `/v1/models` is not mounted.

| Provider | Prefix | Supported surfaces |
| --- | --- | --- |
| OpenAI | `/openai/v1` | Models, Chat Completions, Responses, embeddings, speech, images |
| Anthropic | `/anthropic/v1` | Models, Messages |
| Gemini | `/gemini/v1beta` | Models, text, embeddings, image and speech generation |
| Gemini OpenAI alias | `/gemini/v1beta/openai` | Chat Completions, embeddings |
| Ollama | `/ollama/api` | Tags, chat, embeddings |

| Model | Behavior |
| --- | --- |
| `eliza-1966` | Classic ELIZA/DOCTOR rule-based chat |
| `flite` | Retro speech, with supported MP3, WAV, and PCM formats |
| `fnv-embed` | Feature hashing, 1–1024 dimensions; not semantic embeddings |
| `eliza-retro-image` | Prompt-seeded cellular automata rendered as PNG |

OpenAI and Anthropic stream SSE; Ollama streams NDJSON. Gemini's native streaming
uses its documented JSON-array/SSE variants. Use the exported OpenAPI to inspect
accepted request fields and error envelopes; provider compatibility is a subset.

Text endpoints support structured JSON and one deterministic tool call at a time.
With an offered tool named `echo`, the latest user message
`@tool echo {"value":"hello"}` selects that tool. This tests a client tool round trip,
not autonomous planning. Image input produces coarse pixel statistics, not OCR
or object recognition. Remote image URLs and provider file references are not fetched.

Unknown object fields may be tolerated; unsupported provider variants are rejected.
Successful fixture tests do not replace occasional tests against real providers.
