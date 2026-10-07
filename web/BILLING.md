# Billing and accounts (test mode)

Live payments are deliberately rejected in this slice. Hosted protected capacity,
launch/legal details, deployment and Rust admission are still pending. Enabling
the private billing flag opens **test** login/checkout/key management; the UI says
so. Public browsing remains signup-free with no Supporter promotion in navigation.

## Configuration

Keep these in the platform secret store, never `NUXT_PUBLIC_*`, source control,
public documentation exports or browser bundles:

| Variable | Purpose |
| --- | --- |
| `NUXT_BILLING_ENABLED=true` | Enable test billing; defaults off |
| `NUXT_BILLING_ORIGIN` | Fixed HTTPS site origin; HTTP loopback allowed locally |
| `NUXT_DATABASE_URL` | Managed PostgreSQL URL with required TLS in hosting |
| `NUXT_STRIPE_SECRET` | `sk_test_…` only; Stripe SDK 23 / API `2026-09-30.endive` |
| `NUXT_STRIPE_PRICE` | Test recurring price: USD 500 cents / one month / licensed |
| `NUXT_STRIPE_WEBHOOK_SECRET` | Signing secret for this test-mode destination |
| `NUXT_SMTP_URL`, `NUXT_MAIL_FROM` | Transactional mail transport and verified sender |
| `NUXT_ENTITLEMENT_SECRET` | Random internal read credential, at least 32 characters |
| `NUXT_REPAIR_SECRET` | Separate random scheduled-repair credential, at least 32 characters |

Use different database, email destinations, Stripe objects and credentials for
each environment. Use a local SMTP capture server or a test-only email inbox;
never test against real customers. SMTP must use verified TLS in hosting. Stripe
uses hosted Checkout/Portal: no card data is collected here. Set the portal to
allow cancellation/payment-method updates, disable plan changes and trials, and
verify applicable tax treatment before the separate live-launch change.

Run `NUXT_DATABASE_URL=… pnpm --dir web migrate:billing` once before enabling the
service. The schema is transactional and repeatable, guarded by an advisory lock.
Runtime does not create tables on cold starts. Use separate migration/runtime
database roles: runtime needs only CRUD, not schema permissions. PostgreSQL is
the source of truth; the four-connection pool is only a connection optimization.
Use the provider's pooled URL for serverless hosting; choose provider/region in
PR 019. No queue, local state file or Redis service is required.

## HTTP contracts

`/api/**` remains reserved for the Rust API. Browser mutations require an exact
configured `Origin`; the browser supplies it. JSON account bodies are bounded to
4 KiB. Account/billing responses have `private, no-store`; sign-in/account pages
are `noindex`. Do not cache these routes at any upstream proxy. Enforce request
size and client-IP login limits at the deployment edge too (PR 019): the fallback
socket IP intentionally does not trust arbitrary forwarded headers.

- `GET /account/state`: availability, signed-in account, eligibility and key IDs;
  no hashes or plaintext keys.
- `POST /account/login {email}`: issue a 15-minute single-use email link, limited
  to 3/email and 10/socket-IP per 15 minutes. Email tokens are SHA-256 hashes at rest.
  The link holds the token in the fragment, not an access-log query parameter.
  GET/email scanners cannot consume it. An explicit confirm button POSTs it.
- `POST /account/confirm {token}`: atomically consume link and create a seven-day
  HttpOnly, SameSite=Lax session (Secure on HTTPS). Top-level returns from hosted
  Stripe keep the session; mutations still require an exact Origin. Session hash stored in DB.
- `POST /account/logout {}`: revoke that session and clear its cookie.
- `POST /account/keys/create {label}`, `/keys/rotate {id,label}`, `/keys/revoke {id}`:
  owner-only. Creation requires current paid/grace eligibility; revocation remains
  possible after expiry. Random 256-bit secrets are shown once, never emailed.
  Rotation keeps the old key for **at most 24 hours**. Update CI secrets, verify
  the new key, then revoke the old one immediately. At most 20 usable keys/account.
- `POST /billing/checkout`, `/billing/portal`: authenticated owner, server-selected
  price/customer/return URLs; checkout limited to 5/account per 15 minutes. Open
  Checkout sessions are reused; account locking and Stripe idempotency avoid
  duplicate creation on retries. Existing subscriptions go through the portal.
- `POST /billing/webhook`: raw body + `Stripe-Signature`, bounded to 1 MiB. Live
  notifications rejected. Store only event/customer IDs, acknowledge after durable
  insertion, deduplicate by event ID. Invalid signatures never enter the database.
- `POST /billing/repair`: separate bearer credential; schedule **every minute**.
  Processes 25 due events and reconciles 25 due accounts per call. Provider failures
  back off from 30/60 seconds to one hour; unfinished work stays durable. Monitor
  pending age and oldest account reconciliation age; increase repair cadence or
  batch capacity when those approach the agreed bounds. Scheduler/alert wiring
  and platform execution budgets must be validated in PR 019 before paid launch.
- `POST /billing/entitlement`: internal bearer credential; body is the 64-character
  hex SHA-256 of the supplied API key. Never put a key/hash in a URL. Returns
  eligibility, account ID, eligibility end and cache bounds. This read contacts
  only PostgreSQL, never Stripe. Reject unknown/revoked/expired keys.

All errors redact underlying SMTP, database and Stripe errors. Configure hosting
logs to exclude account/billing bodies, cookies, Authorization and webhook payloads.
Do not log one-time tokens or returned keys. Alerts use counts/event IDs only.

## Eligibility and recovery

Reconciliation locks the account **before** retrieving current Stripe subscriptions
and paid invoices. It ignores notification arrival order. Only a paid USD invoice
with at least $5 received and a matching $5 subscription line can advance paid-through
access. Checkout redirects, active-but-unpaid subscriptions and incomplete/trial
subscriptions cannot grant initial access. Paid-through dates never move backwards.
Cancellation retains paid-through access. A previously paid `past_due`/`unpaid`
subscription receives seven days after paid-through; retries do not reset grace.
Canceled/removed subscriptions lose grace. Key revocation is independent and is
never reset by billing events. Refund/dispute/operator-suspension policy is a
separate launch decision; no automatic account suspension is implied here.

The repair endpoint also rescans customers even without a webhook, recovering
missed deliveries. A crash rolls processing back; another invocation can claim
the pending record. Database/provider failures do not acknowledge undurable work.
After database restore, reconcile before enabling traffic and re-send Stripe
events newer than the backup. Restoring old key rows can undo revocations, so do
not serve a restored database until incident-time revocations have been reapplied.

PR 017 implements the Rust cache: normal refresh/revocation target **60 seconds**;
known eligible entries may use at most **300 additional seconds** of stale grace
during a read outage, never beyond their paid/grace/key-expiry end. Unknown keys
never upgrade on outage; cached denials do not become approvals. This slice
defines/provides the read contract, **not** a tested generation outage guarantee.

## Verification

Use a disposable database with a name ending in `_test`. Tests truncate it.

```sh
export TEST_DATABASE_URL=postgres://postgres:local-test-only@127.0.0.1:18879/eliza_test
pnpm --dir web test:billing
pnpm --dir web check
pnpm --dir web build
pnpm --dir web test
```

Service tests use real PostgreSQL with a simulated Stripe transport and test mail
capture callback. Browser/HTTP tests run both disabled preview and enabled local
test billing, check ownership/Origin/session controls and signed raw-body webhooks,
and exercise one-time keys/rotation/revocation on desktop and phone. No external
email or payment is sent. Run `tools/check-billing-restore.sh` with the dedicated
local Docker test container to verify backup/restore.

Still required with actual **Stripe test-mode** credentials: create the price,
configure the portal and signed destination, complete Checkout with Stripe test
cards, verify renewals using test clocks, failed renewal/grace/cancellation, replay
duplicates/out-of-order deliveries, and kill processing then invoke repair. Verify
transactional email delivery with the chosen provider and restoration through the
managed database provider. These are launch gates, not simulated test claims.

Implementation follows [Stripe raw-body signature guidance](https://docs.stripe.com/webhooks),
[subscription objects](https://docs.stripe.com/api/subscriptions/object), and
[PostgreSQL row locking](https://www.postgresql.org/docs/current/explicit-locking.html).
