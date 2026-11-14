# Changelog

## v1.0.0

First public release of ELIZA as a repeatable, local fixture for AI client development and contract tests.

- Serve the 1966 ELIZA conversation engine through OpenAI, Anthropic, Gemini, and Ollama compatible HTTP routes.
- Exercise streaming, structured JSON output, tool calls, reasoning traces, and vision with deterministic responses.
- Test local speech, embeddings, and image generation without model downloads, credentials, or upstream calls.
- Run through Docker, Nix, standalone Linux and Windows executables, macOS archives, or Debian and Alpine packages.
- Download the Nix-built Docker image archives for Linux AMD64 and ARM64, or pull the multiarchitecture image from `ghcr.io/shorwood/eliza:1.0.0`.
- Verify release downloads with the SHA256 digests provided by GitHub.

See the [README](https://github.com/shorwood/eliza#readme) for the API scope and quick start.

### Release repair

- Preserve the original v1.0.0 Git tag and identify the replacement build's source in `SOURCE.json`.
- Include required notices in packages, containers, and portable archives; retain bare executables with companion notice archives.
- Provide Linux tar.gz and Windows zip archives, per-download checksums, and `SHA256SUMS`.
- Bound shared image workers across providers without blocking async HTTP work, including after client cancellation.
- Update affected Rust dependencies and run recurring dependency audits.
