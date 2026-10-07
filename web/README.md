# Nuxt site and public documentation

`app/` contains the Nuxt landing, rendered docs, support/account preview, and
terms/privacy publication notes. `server/billing/` implements test-mode accounts,
subscriptions and team keys; see [BILLING.md](BILLING.md). `content/docs/` contains public Markdown;
`public/examples/` contains independent SDK/CI examples. Hosting and billing are
separate implementation slices; live payments and hosted key admission are disabled.

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web dev
pnpm --dir web check
pnpm --dir web build
pnpm --dir web exec playwright install chromium
# Disposable PostgreSQL database; see BILLING.md for setup.
export TEST_DATABASE_URL=postgres://postgres:local-test-only@127.0.0.1:18879/eliza_test
pnpm --dir web test:billing
pnpm --dir web test
```

Use Node.js 22.22.3+ on the supported 22.x line, or a Nuxt-supported newer version.
The Nix development shell supplies a compatible Node/pnpm. Browser tests run the
production server and cover desktop/phone, clipboard recovery, no-JavaScript
reading, hidden support offers, private cache headers, fake activation links,
and exclusion of SDK install/build artifacts. On NixOS, set
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` to your installed Chromium path if the downloaded
browser cannot run. CI installs Chromium and OS dependencies normally.

## Rendering and configuration

Public pages are SSR/prerendered. Repository Markdown is converted and sanitized
at build time, with links rewritten to rendered routes and raw `.md` downloads.
Do not build and run the dev server concurrently in the same `.nuxt` directory.
Support/account pages remain dynamic, `private, no-store`, and `noindex`; they
collect no email/key while billing is disabled and ignore purported activation
state in query parameters. Enabling private test billing opens passwordless
sessions, checkout/portal and key management. Account and subscription state comes
from PostgreSQL; checkout redirects cannot assert payment success.

Set public configuration before the production build (and consistently at runtime):

- `NUXT_PUBLIC_API_ROOT`: instance root before provider prefixes; defaults to a
  local instance. It must never contain credentials. Hosted `/api` belongs here.
- `NUXT_PUBLIC_SERVICE_LIVE`: enable only after public hosting is validated.
- `NUXT_PUBLIC_SITE_URL`: the actual site origin for canonicals and sitemap. Without
  it, no invented canonical is emitted and robots disallows indexing.
- `NUXT_PUBLIC_STATUS_URL`: independent monitoring URL; otherwise status links to
  the publication note. No uptime claim or healthy badge is fabricated.

There is no landing-page link, footer promotion, or sitemap entry for `/support`.
Rate-limit errors will supply its URL when admission is implemented. Direct support
access shows the $5 plan and disabled checkout by default, or explicitly labeled
test checkout when configured. No generation request is
sent by browsing pages or changing providers.

## Public assets

Nuxt serves `generated/public`, created by `tools/prepare-site.mjs` from the shared
public example allowlist. It never recursively publishes `public/examples`:
SDK node_modules and Cargo targets must not become downloadable assets. Generated
HTML/JSON/assets are ignored source artifacts. `public/openapi.json` is the schema
from public docs preview `64b40e2`; regenerate it from the documented release when
the hosted contract changes. It is marked as a preview and makes no live API claim.

## Documentation export

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
