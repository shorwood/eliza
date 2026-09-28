# Client examples

These examples exercise ELIZA through real provider libraries. Each command
builds and starts the server, runs every SDK-native example, and stops the
server afterward.

```sh
nix develop -c just example-vercel-ai
nix develop -c just example-rust-rig
```

Set `ELIZA_EXAMPLE_PORT` to use a port other than `8787`.

The Vercel AI example covers OpenAI Chat Completions and Responses, the Gemini
OpenAI-compatible alias, Anthropic Messages, native Gemini generation, and tool
round trips for each native tool-capable surface. The Rig example covers the
generation routes plus OpenAI, Gemini, and Ollama model listing and native
Ollama chat. Vercel AI does not expose provider model-listing operations, so the
TypeScript example does not replace them with raw HTTP calls.
