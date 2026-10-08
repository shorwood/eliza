# Hosted service contract

This document defines the launch requirements for the hosted ELIZA service.
The service is not live yet; the capabilities below are implementation commitments,
not claims about the current server. Public terms and privacy pages must reflect
the deployed behavior and identified seller before live subscriptions begin.

## Purpose

ELIZA is a hosted development fixture for small startups, individual builders,
and teams testing AI SDK integrations. Developers point an existing SDK at the
documented provider base URL to exercise supported HTTP envelopes, streaming,
tool calls, and synthetic modalities without real model inference charges.

The chatbot uses ELIZA/DOCTOR; speech uses Flite; embeddings use feature hashing;
images are retro procedural fixtures. Documentation must describe the supported
provider subset and unsupported variants. Passing these tests does not establish
model quality or compatibility with every behavior of a real provider.

The implementation repository is private. Public HTML/Markdown documentation,
OpenAPI, standalone SDK examples, CI examples, changelog, and service status remain
accessible without repository access. Previously distributed MIT copies retain
their license; repository privacy does not retract those permissions.

## Access and price

| | Public | Supporter |
| --- | --- | --- |
| Signup | None for API access | Account for billing/key management |
| Price | Free | $5 USD per month, recurring |
| Text-request quota | No daily or monthly quota | No daily or monthly quota |
| Supported capabilities | All documented capabilities | The same capabilities |
| Admission | Generous IP-based rates and bursts | Higher account-based rates and bursts |
| CI access | Shared public capacity | Team key and protected supporter capacity |
| Fixture pinning | Available | Available |
| Media | Published separate allowances | Larger published allowances |

One subscription covers a small team's development and CI use. Do not charge
per seat, token, or text request. Multiple keys and rotation overlap share the
account's budget; creating keys does not multiply its allowance. No enterprise
tier, custom sales process, or organization administration is needed at launch.

Checkout must show the recurring amount, currency, applicable taxes, final payable
total, and cancellation terms before confirmation. Advertised tax presentation
must match the seller's actual configuration. The subscription funds service
operation and confers benefits; do not advertise it as a tax-deductible donation.
No automatic usage overages or undisclosed additional charges.

## Support discovery

The landing and ordinary documentation lead directly to useful API calls. No
pricing cards, support banner, upsell footer, mandatory signup, or promotional
text in successful generated responses. Existing supporters can visit `/account`
directly; `/support` is reachable when someone requests it.

Only an anonymous request that exceeds its genuine tier rate, burst, concurrency,
or media allowance may advertise the supporter option. Use the provider-native
429 envelope and an accurate `Retry-After` header. Example copy, with the actual
retry time and deployed domain substituted:

> Public rate limit reached. Retry in 12 seconds. Need more capacity for CI?
> Support the service for $5/month: https://example.com/support

Never reduce public capacity artificially to encourage payment. Supporter limits
receive factual retry guidance without an upsell. Global overload, outages, and
invalid credentials do not advertise payment as a remedy. Never insert an offer
into generated text or an already-started successful stream.

## Usage limits

Unmetered text means no period request quota, within published sustained rates,
burst sizes, concurrency, request/output bounds, and maximum stream duration.
It does not grant unbounded resource use or an unrestricted load-testing service.
Ordinary development and CI should not require counting monthly requests.

Speech and image allowances are separate because computation and output bytes
vary. Publish their rates, concurrency, input/output bounds, periodic allowance
if used, accounting unit, and reset time. Exhaustion rejects new media work with
an actionable retry time; text remains available within its own limits. Renewals
do not silently create a usage charge. Do not claim unlimited media delivery.

Supporter limits follow the account across its keys. Public IP limits account for
bursts and shared networks, with documented retry behavior. Arbitrary client keys
and spoofed forwarding headers cannot create extra budgets. Bound admission state,
queues, generated work, and stream lifetime so overload remains recoverable.

Concrete launch settings must be measured on the deployed HTTPS path and published
before beta. Local benchmarks do not establish production availability or a
guaranteed concurrent-user count. Changes to published limits require a changelog;
temporary emergency restrictions must be visible on status with an explanation.

## CI stability and availability

Protect supporter admission and expensive-work capacity from anonymous traffic.
Both tiers can pin a fixture release independently of the provider's own API
version. Pinning freezes the documented deterministic behavior and supported
contracts; it does not freeze timing, operational headers, or transient failures.

Do not silently replace or redirect a pinned release to changed fixture behavior.
Publish supported versions and retirement dates, with at least 90 days' retirement
notice. Start with a bounded set of hosted versions rather than promising indefinite
support. Reject a retired version explicitly and provide migration instructions.

Run compatibility checks before deployment, retain rollback capability, and drain
in-flight work where possible. Monitor the public HTTPS path externally. Status
must remain accessible during a main-site/origin outage. Publish incidents and
measured uptime with its observation period; do not promise a contractual SLA,
uninterrupted workflows, or support response times not backed by operations.

CI examples use secret storage, a pinned fixture, bounded retries respecting
`Retry-After`, and explicit timeouts. A hosted fixture is an external dependency:
applications still need to handle temporary failure and cancellation.

## Billing and credential lifecycle

Confirmed payment activates supporter eligibility. A successful checkout redirect
alone does not. Duplicate, delayed, or reordered payment events must not duplicate
accounts, resurrect revoked credentials, or shorten a paid-through period.

Cancellation stops renewal and retains access through the paid period. A failed
renewal receives seven days of payment grace, shown in the account with recovery
instructions. After grace expires, supporter benefits end without extra usage
charges. The public endpoint remains usable under public access rules.

Generate random keys, reveal them once, and store hashes rather than raw secrets.
Support authenticated creation, explicit revocation, and rotation with overlap so
a team can update CI secrets without an immediate interruption. Publish the overlap
duration before launch. Revoked credentials are not silently treated as anonymous.

Generation does not synchronously depend on Stripe. Cache known eligible keys
with an explicit refresh and outage policy; revocations should propagate within
60 seconds in healthy operation. Before launch, select a bounded stale-access
window and test database/billing outages. Never grant supporter access to an
unverified new key because a dependency is down. Publish how outage behavior affects
revocation and access rather than implying immediate revocation under all conditions.

## Data and acceptable use

Do not persist prompts, histories, generated audio/images, raw API keys, or
authorization headers in application logs. ELIZA conversation state remains
request-local. Retain only account/contact details, necessary billing identifiers,
key hashes, entitlement state, and bounded operational metadata.

Before beta, record and publish retention periods for application records,
limiter IP state, ingress/provider logs, monitoring, backups, and accounting data.
Identify the seller, processors, hosting regions, deletion/contact procedure, and
any legally required retention that survives account deletion. The privacy page
must match infrastructure configuration; do not promise zero retention while
an ingress provider keeps access logs. Avoid optional tracking at launch.

Development and CI for commercial projects are welcome. Do not permit credential
sharing as a resale service, deliberate exhaustion, evasion of published limits,
or attempts to access another account. Enforcement must address observed behavior
and explain recovery where feasible. Safeguards must not turn normal free use
into a forced upgrade.

## Launch acceptance

- Public calls work without signup, with useful rates and no text-period quota.
- Both tiers support the same documented capabilities and fixture pinning.
- Support promotion appears only on genuine anonymous tier-limit errors.
- Checkout shows $5/month and the actual total/renewal/cancellation terms.
- Keys, paid-period cancellation, seven-day payment grace, and media resets work.
- Anonymous load cannot exhaust all supporter text or expensive-work capacity.
- Version promotion, retirement notices, outage handling, and rollback are tested.
- Public docs, independent status, terms, privacy, contact, and retention are ready.
- Deployed benchmarks establish concrete limits and a sustainable operating budget.

Launch inputs still required: production domain, seller identity/tax setup,
Rust-origin host, Vercel commercial plan, database and transactional email provider,
launch limits/media allowances, supported-version window, key rotation overlap,
outage cache window, retention periods, and operating budget. `example.com` is a
placeholder. These deployment inputs do not block implementing docs, UI, or test-mode
billing, but must be settled before their corresponding live promises are published.
