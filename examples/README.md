# Client examples

These examples exercise ELIZA through real provider libraries. Each command
builds and starts the server, runs every SDK-native example, and stops the
server afterward.

```sh
nix develop -c just example-vercel-ai
nix develop -c just example-rust-rig
```

Set `ELIZA_EXAMPLE_PORT` to use a port other than `8787`.

Each entrypoint is split by capability, so an individual request can be read
without first understanding an example framework. Together they cover:

| Capability | Vercel AI SDK | Rig |
| --- | --- | --- |
| Chat and streaming | OpenAI Chat and Responses, Gemini alias, Anthropic, Gemini | The same surfaces plus Ollama |
| Tool round trips | OpenAI Chat and Responses, Anthropic, Gemini | — |
| Reasoning traces | OpenAI Responses, Anthropic, Gemini | OpenAI Responses, Anthropic, Gemini, Ollama |
| Image input | Every chat surface above | Every chat surface above |
| Embeddings | OpenAI, Gemini alias, Gemini | The same surfaces plus Ollama |
| Image generation | OpenAI, Gemini | OpenAI, Gemini |
| Speech | OpenAI, Gemini | OpenAI |
| Model listing | — | OpenAI, Gemini, Ollama |

The examples stay within each SDK's native features. In particular, Vercel AI
does not expose provider model-listing operations, so the TypeScript example
does not replace them with raw HTTP calls.
