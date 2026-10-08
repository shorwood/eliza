# Hosted execution measurements — 2026-10-08

These local measurements exercise staging admission and protected work. They do
not establish production capacity, uptime or an unlimited-service promise.

## Revisions and method

- Baseline: `1f404da3b1bb3c8e6282096b4860373acc1b94f6` (017A).
- Candidate: `ad1aafbcb19357d0f6d52c6324821bc0de3ee13a` (017B audited runtime).
- Baseline executable SHA-256: `51b3020c9cbde75e3a3e5fce0d02dd5082adc5268308abd4ade91886a26b30c3`.
- Candidate executable SHA-256: `fc82c882e5fb04012895beb94641f177ed08e87663377c4a7337bac0124093f5`.
- Driver: `ad1aafb` for the first baseline point, `3d85cfa` thereafter. Only
  Nix certificate-store packaging changed between those driver revisions.
- Rust `1.97.0-nightly`, commit `e8e4541ff19649d95afab52fdde2c2eaa6829965`, LLVM 22.1.2.
- Reported unstable i9-14900K; server E cores 16–23, client E cores 24–31.
  Eight async workers and two speech workers; release builds with locked dependencies.
- Fresh independent origin per point. Two-second warmup, ten-second measurement,
  three independent rounds. Revision order A/B, B/A, A/B. Both trees were clean.
- Fixed nine-chat/one-medium-PCM-speech corpus, 10% supporter requests, 100 public
  identities, actual local Nitro/PostgreSQL entitlement reads and a confirmed test key.
- `benches/hosted-stress.json`: personal quotas raised to isolate shared saturation.
  Shared reservations and byte/deadline bounds retain the staging values. Baseline
  implements admission but does not yet enforce the candidate's work/media bounds.
- Independent open-loop arrivals retain generator drops. Closed-loop clients replace
  completed requests immediately. Summary values below are medians across runs;
  latency is the median of each run's successful per-tier percentile, not a pooled percentile.

Compact per-run results are in [017-results.json](017-results.json). Raw samples,
HDR histogram bins, process snapshots, full policies, lockfiles and metadata remain
in `/tmp/eliza017-final-wire-{baseline,candidate}-r{1,2,3}` on the development machine.
An earlier run overlapping compilation was discarded. Subsequent CI-only cleanup
and report commits do not change the measured runtime.

## Results

| Point | Metric | 017A | 017B |
| --- | --- | ---: | ---: |
| Open, 200 offered requests/s | Accepted requests/s | 199.9 | 199.9 |
| Open, 200 offered requests/s | Server CPU seconds / 10-second window | 5.73 | 6.20 |
| Open, 200 offered requests/s | Supporter successful p95, ms | 52.959 | 53.183 |
| Open, 200 offered requests/s | Shared rejections / window | 0 | 0 |
| Closed, 32 clients | Accepted requests/s | 1798.9 | 662.5 |
| Closed, 32 clients | Public successful p95, ms | 156.415 | 48.031 |
| Closed, 32 clients | Supporter successful p95, ms | 155.391 | 78.911 |
| Closed, 32 clients | Shared rejections / window | 0 | 56236 |
| Closed, 32 clients | Peak sampled origin RSS, MiB | 193.340 | 159.777 |
| Closed, 32 clients | Server CPU seconds / window | 56.05 | 77.45 |
| Closed, 32 clients | Received body bandwidth, MB/s | 87.698 | 18.520 |

Every comparative run had zero unexpected failures and zero dropped client arrivals.
At equal offered load, the execution boundary adds about 8% CPU. Under fixed-client
flooding, bounded execution accepts fewer requests, halves supporter successful p95
and lowers sampled RSS. Fast 503 responses cause clients to offer much more work;
that closed-loop CPU total does not compare equal offered workloads. The four-job
CPU budget bounds admitted heavy work, not total process CPU spent handling ingress
and rejections. Successful latency excludes rejected requests; raw samples and
status counts retain them.

## Controls and additional cases

Controls use runtime `50a09df9a42483c58f8e41980221e86dcb7498a8`, before the image
classification and alternate-Serde-form audit fixes, with the same corpus and affinity, two-second warmup,
five-second windows and three repetitions. At 200 offered requests/s, local mode,
hosted anonymous and hosted cached supporter each completed 199.8 requests/s with
zero failures, drops or shared rejections. Median CPU/window was 2.09, 3.15 and 3.20
seconds respectively. Their p95 was 52.063, 53.119 and 53.119 ms. These short controls
show the full hosted-policy overhead separately from the execution-only comparison.

Six first supporter requests recorded during warmup took 7.49–16.90 ms on loopback.
They include cold entitlement lookup, HTTP and generation; they are not pure lookup
microbenchmarks or production network estimates. One-second whole-lookup deadlines,
coalescing and outage clocks are also checked by deterministic Rust tests.

Other diagnostic runs used candidate `da7baadd00dee63cc57a2280f230c94bb638245c`, before
the native embedding-preflight audit fix, with driver `f34024734a888eaff1ec46ebfa002a35e25acf06`:

- Exact staging-policy chat, speech, image, embedding and inline-image smoke: zero
  unexpected failures and drops; personal limits recorded separately.
- Paced 1 KiB uploads, 50 ms/chunk: all 30 open-loop requests completed, no drops or
  transport failures. Native shared rejections occurred in the closed flood.
- Speech readers delayed 50 ms/chunk: all 30 open-loop requests completed, no drops
  or transport failures. Completion extended into drain; successful-in-window RPS
  alone would incorrectly describe these as unserved.
- 131072 synthetic verified public identities, 9000 offered requests/s for 20 seconds:
  28947 generator drops, 7851 transport failures/timeouts, 2245 complete successes,
  140957 shared rejections and peak sampled origin RSS 836.406 MiB. This overloaded
  generator/server case is rejected as capacity evidence. It exposes the cost of
  pre-admission connections and control-plane traffic; admission tables remain
  bounded, but response/work permits do not cap every accepted socket.

Raw diagnostics are under `/tmp/eliza017-{high-cardinality,slow-readers,slow-uploads}`;
controls under `/tmp/eliza017-control-{local,public,supporter}`. These are local artifacts,
not permanent hosted URLs. No credentials are recorded in samples or report metadata.

Actual TCP tests stop consuming an unread 32 MiB response and verify that the absolute
socket deadline releases delivery and work. Separate cancellation barriers verify
that started native workers retain CPU/input ownership after HTTP cancellation.
State tests fill all 100000 public entries, 10000 account entries and 10000 retained
cache entries; new admission fails closed while existing accounting survives.
Provider fixtures verify shared OpenAI/Gemini speech, implicit Gemini image models,
inline-image charging, encoded paths, alternate Serde wire forms and native 429s
without promoting support to paid callers. Image-like tool schemas remain text.
Real Nitro/PostgreSQL key creation, activation and warm-key revocation also pass.

## Production gates

PR019 must measure sustained traffic on stable deployment hardware, tune async and
blocking worker counts, enforce ingress connection/header/rate bounds, and measure
worst-case parsed JSON/media memory as well as raw input buffers. It must establish
HTTPS, ingress identity and direct-origin isolation and price actual bandwidth.
The high-cardinality diagnostic and hosted overhead make these explicit launch gates.
Live billing remains disabled pending that phase. These results justify protected
staging admission; they do not justify publishing a concurrent-user count.
