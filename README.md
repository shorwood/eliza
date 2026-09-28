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
for this server. This command builds and serves ELIZA for the lifetime of the
chat session:

```sh
nix develop -c just chat
```

Runnable Vercel AI and Rig compatibility examples live in
[examples](examples/README.md).

The flake also publishes the server as a Nix package. Run it directly from a
published flake reference with:

```sh
nix run <flake-url> -- serve --port 8787
```

To add ELIZA to another flake, declare it as an input and select its package for
the consuming system:

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    eliza = {
      url = "<flake-url>";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, eliza, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = [ eliza.packages.${system}.default ];
      };
    };
}
```

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
curl -s http://127.0.0.1:8787/openai/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-doctor","messages":[{"role":"user","content":"I am sad."}]}'
```

The Gemini OpenAI-compatible alias is also available:

```text
http://127.0.0.1:8787/gemini/v1beta/openai/chat/completions
```

## Anthropic Messages

```sh
curl -s http://127.0.0.1:8787/anthropic/v1/messages \
  -H 'Content-Type: application/json' \
  -H 'anthropic-version: 2023-06-01' \
  -d '{"model":"eliza-doctor","max_tokens":128,"messages":[{"role":"user","content":"I am sad."}]}'
```

## Gemini Generate Content

```sh
curl -s http://127.0.0.1:8787/gemini/v1beta/models/eliza-doctor:generateContent \
  -H 'Content-Type: application/json' \
  -d '{"contents":[{"role":"user","parts":[{"text":"I am sad."}]}]}'
```

## Ollama Chat

Ollama streams newline-delimited JSON unless `"stream": false` is supplied:

```sh
curl -s http://127.0.0.1:8787/ollama/api/chat \
  -H 'Content-Type: application/json' \
  -d '{"model":"eliza-doctor","stream":false,"messages":[{"role":"user","content":"I am sad."}]}'
```

Model catalogs are available at `/openai/v1/models`, `/anthropic/v1/models`,
`/gemini/v1beta/models`, and `/ollama/api/tags`.

## Deterministic tool calls

Every chat surface accepts client-defined function tools. Tools do not change
ordinary ELIZA prompts. To request one call, make the complete latest user
message `@tool <offered-name> <json-object>`, for example:

```text
@tool echo {"value":"hello"}
```

ELIZA returns that call in the selected provider's native shape. Submit the
provider-native tool result in the next request and ELIZA replies
`TOOL CALL COMPLETE`. The fixture is stateless, supports one call at a time,
and does not validate arguments against the supplied JSON Schema.

With `--auth bearer`, OpenAI, Gemini's OpenAI alias, and Ollama use
`Authorization: Bearer`; Anthropic uses `x-api-key`; native Gemini uses
`x-goog-api-key`.

Streaming surfaces return provider-shaped SSE events where provider SDKs expect
them, while Ollama uses NDJSON. The engine still computes one deterministic
ELIZA response and splits it into chunks. This is a historical chatbot adapter,
not a medical or therapeutic system.
