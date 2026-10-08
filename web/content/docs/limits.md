# Limits, retries, and fixture versions

Current instances bound text, message history, decoded media, structured schemas,
and generated output. Operators can configure input/history bounds. Read the
instance's deployed contract; hosted text/media allowances are not selected yet.

The hosted design has no daily/monthly text-request quota. Published rates,
bursts, concurrency, input/output bounds, and separate media allowances will still
apply. These are planned policies, not working rate-limit or billing features.
No automatic usage overages are planned.

For a genuine tier limit, the hosted API will use provider-native HTTP 429 errors
with `Retry-After`. Wait for the given interval, use bounded retries with jitter
where appropriate, and limit simultaneous CI requests. Invalid credentials and
validation errors need correction rather than repeated retries. Global overload
is distinct from exceeding an account allowance.

For streams, set a deadline and cancel work when no longer needed. Do not silently
concatenate a retried partial stream to its first attempt. The curl smoke script
does not retry streaming requests for that reason.

Hosted version pinning is planned. Current local `/openai/v1` and other provider
versions are compatibility-route segments, not immutable fixture versions. When
pinning is available, documentation will list supported fixture releases and
retirement dates with at least 90 days' notice.
