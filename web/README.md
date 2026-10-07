# Public documentation sources

`content/docs/` contains public Markdown; `public/examples/` contains independent
SDK/CI examples. This is not yet a Nuxt application. HTML routes, canonical URLs,
and the sitemap follow in the site slice; deployment follows separately.

Export a new, empty directory for publication using Node.js 22+ and the generated
OpenAPI from a running instance:

```sh
node web/tools/export-docs.mjs \
  http://127.0.0.1:8787/openapi.json \
  https://example.com/api \
  target/public-docs
```

The placeholder API root illustrates hosted rewriting; choose the actual deployed
root before publication as a live API. OpenAPI retains provider paths, with `/api`
in its server URL so clients do not lose or duplicate the mount prefix. Regenerate
from the release being documented; do not manually invent request schemas.

Export permits only public Markdown and a fixed list of example files. SDK locks
are included; node_modules, targets, private source, proposal notes, account/billing
implementation, secrets, and git history are excluded. Use a fresh destination to
avoid retaining unrelated files from a previous export. Review and secret-scan the
bundle before publishing it to the public docs-only repository.

`PUBLIC-LICENSE` applies only to these public docs and standalone examples. Public
service/legal pages currently describe pending publication, not a launched service.

Run `nix develop -c just check-public-docs` to check shell syntax, SDK type checks,
Rust formatting/Clippy/Dylint, actual completions/streams/errors, and the exported
OpenAPI/Markdown bundle against a temporary local server. This checks current
behavior; production HTTPS and hosted fixture pinning await deployment.
