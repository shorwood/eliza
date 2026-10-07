# Hosted staging policy

Hosted mode is opt-in:

```sh
eliza serve --bind 127.0.0.1:8787 --hosted-config config/hosted-staging.json
```

Supply independent `ELIZA_INGRESS_SECRET` and `ELIZA_ENTITLEMENT_SECRET` values through the deployment's secret store. Both require at least 32 bytes. `--api-key` and `--hosted-config` are mutually exclusive. Without a hosted config, local SDK placeholder credentials remain compatible.

This is a **staging profile**, not production sizing. Replace the support URL and restricted entitlement endpoint before deployment. Only loopback development entitlement HTTP is permitted; deployed URLs require HTTPS. No Stripe credential belongs in the Rust origin.

The ingress must remove incoming `x-eliza-ingress-token` and `x-eliza-client-ip`, then set its own token and the verified client IP. The origin also checks the actual immediate peer against `trusted_peers`. Keep the origin inaccessible to arbitrary Internet peers. `Forwarded` and `X-Forwarded-For` never determine identity. IPv4-mapped IPv6 normalizes to IPv4; other IPv6 clients share a /64 allowance.

Provider-native headers carry supporter keys. Ordinary SDK placeholders use the public tier. Malformed, unknown, revoked, expired, and ineligible ELIZA keys never become anonymous requests. Keys are hashed before restricted billing reads; account allowances span all that account's keys.

Eligible reads stay fresh for at most 60 seconds. A successful refresh confirming denial takes effect immediately. During an outage, previously confirmed eligibility survives only until the earlier of its absolute expiry and 360 seconds after its last successful read. Failures cannot reset that clock. Unknown, evicted, and restarted cache state never gains supporter privileges. Lookup deadlines include coalescing waits and response reading; storage, callers, and response bytes are bounded.

Personal allowance failures return native 429 bodies with `Retry-After`. Only anonymous generation allowance failures mention the $5 support option. Shared capacity, bounded-state saturation, and unavailable entitlement reads return retryable native 503 responses without a support offer. Successful generation responses contain no support advertising.

PR 017A implements identity, rate/concurrency admission, and response reservations. The worker, media, input-buffer and output/deadline values in this profile are enforced by stacked PR 017B. Do not treat 017A alone as ready for public deployment. PR 019 owns stable production HTTPS, rollout, calibrated limits and operating costs; live billing remains disabled before launch approval.
