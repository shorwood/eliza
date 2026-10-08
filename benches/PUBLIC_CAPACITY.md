# Public-capacity exploration

The strict zero-miss result is **500 mixed requests/s**, equivalent to 10,000
active users if each submits one request every twenty seconds. This is a traffic
model, not a test of 10,000 simultaneous connections. The server also handled
approximately 1,000 and 2,000 requests/s without failures or latency-budget
violations, with rare missed arrivals in the load generator. Those higher rates
do not satisfy the preselected zero-miss rule.

These local results remain provisional on the reported unstable i9-14900K.
Public TLS, a reverse proxy, Internet latency/bandwidth, idle connections, longer
chat histories, other modalities, and slow streaming consumers were not tested.
Stable-host reproduction and a production-path soak remain necessary for a
deployment guarantee.

## Method

The release server uses the uncommitted performance patches after `f016252`:
shared OpenAPI state and configurable workers. It has four Tokio workers and
four shared speech permits. Server CPUs 16–23 and client CPUs 24–31 are separate
E cores; benchmark threads are assigned only to E cores. File-descriptor soft limits are 16,384 for
both processes. Release LTO and one codegen unit remain enabled.

The corpus offers nine short unary chat requests followed by one medium PCM
speech request. Each point starts a fresh server, warms for ten seconds, and
measures for sixty seconds. There are three independent repetitions, with point
order reversed in the second. No compilation or other benchmark overlaps timing.
Authentication and stream delay are disabled. Connections are reused over
loopback HTTP. Speech uses the existing fixture, `Kore`, and 24 kHz output.

Preselected acceptance requires zero failed requests, zero missed/dropped client
arrivals, both modalities represented, chat p95 at most 100 ms, and speech p95 at
most 5,000 ms. Apply these checks separately to every repetition. The runner's
aggregate mixed latency budget alone cannot establish modality-specific acceptance.
CPU includes all server threads and the final drain. RSS is sampled process
memory, excluding kernel socket buffers. Each latency range below spans the
three per-run quantiles; samples and percentiles are not pooled.

## Simultaneous requests

These are the completed original concurrency sweep under the raised descriptor
limit. Every dispatched request succeeded. Throughput includes the final drain,
which avoids presenting a large initial chat burst as sustained capacity.

| In-flight requests | Requests/s including drain | Chat p95 ms | Speech p95 ms | Peak RSS MiB | Pass all three? |
| ---: | ---: | ---: | ---: | ---: | --- |
| 8 | 1,367–1,374 | 8.72–8.74 | 51.36–51.39 | 19.0–19.6 | Yes |
| 32 | 4,074–4,086 | 8.44–8.62 | 77.38–77.76 | 21.0–21.8 | Yes |
| 128 | 4,069–4,089 | 8.67–8.70 | 312.06–314.88 | 26.5–27.0 | Yes |
| 512 | 4,037–4,090 | 8.66–8.68 | 1,254.40–1,277.95 | 49.6–58.6 | Yes |
| 2,048 | 4,061–4,092 | 8.83 | 5,001.22–5,054.46 | 140.1–149.6 | No |

Above concurrency 32, additional pending requests primarily increase speech
queueing and memory. The 2,048-request level exceeds the speech budget in all
three runs, despite zero request failures. Its in-window throughput appears as
4,368–4,397 requests/s because a large initial burst of fast chat requests finishes
before the slower speech backlog drains. Including drain restores the approximately
4,100 requests/s plateau. Mixed server CPU cost is about 1.08–1.10 ms/request.

The corrected client's concurrency-32 control independently measures
4,084–4,091 requests/s including drain, consistent with that plateau.

## Independent arrivals with the corrected clock

The corrected client has one OS-sleep clock thread and one queued arrival.
Missed queue slots, late dispatch, and occupied client capacity are counted;
delayed arrivals do not become a catch-up request burst. Fixture sequence numbers
include skipped arrivals. Response collection continues while awaiting ticks.

| Offered requests/s | Successful requests/s in window | Chat p95 ms | Speech p95 ms | Mean server CPU cores | Peak RSS MiB | Misses by repetition |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 500 | 499.95 | 3.57–7.59 | 50.91–51.26 | 0.541–0.542 | 18.0–18.4 | 0, 0, 0 |
| 1,000 | 999.80–999.92 | 0.15–7.73 | 50.91–51.14 | 1.080–1.087 | 18.7–18.8 | 2, 0, 7 |
| 2,000 | 1,999.65–1,999.80 | 0.14–8.38 | 50.91–51.17 | 2.165–2.178 | 18.7–19.6 | 2, 2, 12 |

CPU cores means process CPU seconds divided by elapsed seconds; it is usage
across the eight available server cores, not a worker count. CPU/request stays
near 1.08–1.09 ms. Client CPU is below 0.19 cores at 2,000 requests/s, and dispatch
p95 is 15–84 microseconds across these points. Neither average client CPU nor
dispatch p95 indicates sustained generator CPU saturation, but occasional
scheduling misses remain. Miss categories are combined in the saved `dropped`
counter; these measurements do not identify each rare miss's exact cause.

All 90,000 arrivals at 500 requests/s succeed. At 1,000 requests/s, nine of
180,000 scheduled arrivals are missed (0.005%); all 179,991 dispatched requests
succeed. At 2,000 requests/s, sixteen of 360,000 arrivals are missed (0.00444%);
all 359,984 dispatched requests succeed. Worst per-run open-model p99 is 10.02 ms
for chat and 52.03 ms for speech. Requests completed in the drain explain the
small difference between offered and successful in-window rates.

The highest tested strict pass is 500 requests/s. This establishes a tested
operating point, not the server's maximum: higher rates maintain both latency
budgets and zero server failures, but miss the zero-client-drop requirement.

## Translating rates into users and bandwidth

Active users = requests/s × mean seconds between requests. At one request per
twenty seconds, the tested rates correspond to 10,000 / 20,000 / 40,000 active
users. Only the 10,000-user traffic equivalent passes the strict rule in every
repetition. None of these figures establishes simultaneous idle socket capacity.
At one request per five seconds, the same strict rate corresponds to 2,500 users.

One medium PCM response contains 490,716 payload bytes, representing about
10.22 seconds of mono 24 kHz audio. At the offered 10% speech ratio:

| Mixed requests/s | Speech requests/s | Audio payload Mbit/s |
| ---: | ---: | ---: |
| 500 | 50 | 196.3 |
| 1,000 | 100 | 392.6 |
| 2,000 | 200 | 785.1 |

These are decimal payload rates, excluding chat, HTTP, TCP/IP, TLS and proxy
overhead. On a hypothetical 1 Gbit/s public connection, 2,000 requests/s leaves
limited bandwidth headroom even though server CPU is below three cores. Different
text lengths, sample rates, codecs, and speech proportions change this estimate.

## Benchmark findings and retained artifacts

The initial attempt stopped at concurrency 2,048 with `Too many open files` under
the inherited 1,024-file soft limit. Raising both process limits to 16,384 allowed
the complete sweep. The client now records `/proc/self/limits` in its metadata.
The failed attempt remains separate and is excluded from the tables above.

The original open clock used Tokio sleeps and rejected dispatch more than one
arrival period late. At 1,000 requests/s, that period is one millisecond.
The pinned Tokio source documents millisecond sleep granularity; see the
[Tokio timer documentation](https://docs.rs/tokio/latest/tokio/time/fn.sleep_until.html).
The original client consequently delivered only 20–725 requests/s in those
windows, with 16,486–58,821 misses. Even its third 500 requests/s run missed 30
arrivals. Those results remain retained, but are excluded from the corrected-clock
arrival table. They do not establish a server throughput limit.

The replacement uses [OS thread sleeps](https://doc.rust-lang.org/std/thread/fn.sleep.html)
on a dedicated blocking worker. OS scheduling can still oversleep, so late and
missed arrivals remain visible. A stalled-consumer regression test confirms that
the queue retains one tick and counts the remaining arrivals. Five-second checks
delivered all scheduled arrivals at 500 and 1,000 requests/s before sustained
measurements began. `just ok` passed, including workspace tests, all seven HTTP
contract tests, the clock test, Clippy, and Dylint.

Artifacts remain in ignored local directories:

- `target/benchmarks/public-capacity-4x4/`: initial descriptor-limited failure.
- `target/benchmarks/public-capacity-4x4-fd16384/`: original full sweep and clock misses.
- `target/benchmarks/public-capacity-pacer-check/`: brief corrected-clock checks.
- `target/benchmarks/public-capacity-precise-clock/`: sustained corrected-clock follow-up.

Each full comparison retains acceptance policy, metadata, source snapshots,
binary hashes/copies, raw request samples, histograms, process counters, summaries,
and server logs. `capacity-summary.json` preserves ranges and per-repetition
counts. Offered arrivals equal successes + failures + misses, and per-workload
histogram counts match all recorded exchanges in the corrected follow-up.

The server SHA-256 is unchanged across the complete concurrency and corrected-clock
sweeps: `0c5ff0ae0417be6f9c9f84194138108031cbf18f5d72e339bf1cb35552e4331e`.
The corrected client's SHA-256 is
`857d445ead2de7658d75de8707a0663c3cd86053037fa2e5f8a63f6a562048fa`.
No host crashes occurred in these E-core runs. A tool-daemon restart occurred
during the original sweep; its benchmark process survived and continued.

Reproduce the corrected-clock run in the pinned development shell:

```sh
ulimit -Sn 16384
just bench-e2e --runtime-workers 4 --speech-workers 4 \
  --server-cpus 16,17,18,19,20,21,22,23 --client-cpus 24,25,26,27,28,29,30,31 \
  --workloads mixed --concurrency 32 --rates 500,1000,2000 \
  --duration 60 --warmup 10 --repeats 3 --probe-seconds 0 \
  --output target/benchmarks/public-capacity-reproduction
```
