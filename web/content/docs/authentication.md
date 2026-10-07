# Authentication

An unprotected instance accepts requests without credentials. SDKs that require an
API key can use `local` as a placeholder on that instance. A protected instance
requires the key supplied by its operator:

| Surface | Header |
| --- | --- |
| OpenAI, Gemini OpenAI alias, Ollama | `Authorization: Bearer <key>` |
| Anthropic | `x-api-key: <key>` |
| Native Gemini | `x-goog-api-key: <key>` |

The standalone SDK examples read `ELIZA_API_KEY`. Keep real keys in backend
environment variables or CI secret stores; never put them in browser bundles,
source, URLs, or printed command traces. Do not reuse a real provider API key here.

Hosted team keys and account management are planned and not yet available.
Invalid/revoked hosted keys and outage behavior will be documented when those
features are implemented. Current local authentication uses the operator's shared
configured key. Health and generated documentation routes remain public.
