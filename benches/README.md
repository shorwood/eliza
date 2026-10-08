# Eliza benchmarks

All benchmark code lives in this workspace crate. The suite uses Criterion for
generation, Tokio and reqwest for HTTP load, and HDR histograms for latency.
No Python, JavaScript, or
external load-testing service is required.

Recorded measurements: [initial baseline](BASELINE.md),
[PR 003 sharing comparison](PR003.md),
[PR 002 instruction borrowing](PR002.md),
[PR 004 bounded normalization](PR004.md),
[PR 007 scalar key reuse](PR007.md),
[PR 008 borrowed jump labels](PR008.md), and
[public-capacity exploration](PUBLIC_CAPACITY.md).

Enter the pinned shell with `nix develop`, then run:

```sh
just bench-smoke
just bench-generation --save-baseline before
just bench-e2e
```

The full HTTP run takes about four hours: five workloads, twelve load levels,
three repetitions, ten-second warmup, and sixty-second measurement windows.
Generation measurements run separately; do not overlap them with HTTP load or
compilation. CI runs fixture checks and brief HTTP smoke tests without speed
thresholds. `just ok` also checks benchmark targets with Clippy and Dylint.

## Generation

Criterion warms each case for three seconds, measures for ten seconds, and takes
fifty samples. Speech uses flat sampling. Reports and saved comparison baselines
are under `target/criterion/`; preserve that directory when switching commits.
Compare a candidate with `just bench-generation --baseline before`.

| Group | Timed work |
| --- | --- |
| `eliza/compile` | Parse and compile the bundled program and script |
| `eliza/session` | Start a fresh session from an already compiled doctor |
| `eliza/turn/*` | Generate one answer from an independently prepared session |
| `eliza/memory` | Recall queued memory from an independently prepared history |
| `eliza/replay/{1,10,50,100}` | Start a session and replay that many user turns |
| `completion/{ascii,unicode,reasoning}/*` | Normalize, replay, format, and account for a neutral request |
| `normalization/*` | Adapt provider text to the private 72-column terminal record |
| `completion/{long_prefix,long_suffix}/1` | Complete a request with long discarded text around the terminal record |
| `speech/synthesis/*` | Production Flite initialization, synthesis, and resampling |
| `speech/encode/*` | Encode precomputed PCM as PCM, WAV, or MP3 |
| `speech/service/*` | Public service scheduling, synthesis, and complete encoding |

Compilation is outside timed loops except for `compile`. Isolated turns use
borrowed batched sessions, excluding session setup and teardown. Neutral request
construction uses batched setup; completion includes
consuming/dropping the request. Session startup and replay include
cleanup. Encoding and raw synthesis use ordinary timing loops, including output
allocation and destruction. Normalization also includes output allocation and
destruction, with fixture construction and exact output checks outside timing.
Service benchmarks reuse a runtime and clone the request outside timing.
No single-turn benchmark reuses advancing session state.
Returned response/audio destruction is outside timing in batched cases;
ordinary timing loops include destruction of their returned output.

Speech text comes from `fixtures.json`. The baseline uses `Kore`, speed 1.0, no
style, and 8/24 kHz output. Style and multiple segments are separate cases. Each
synthesis call creates an engine, matching production. Criterion's results are
warmed process measurements, not cold-process latency. Fixture setup prints the
sample count and audio duration; real-time factor is the reported generation
seconds divided by those audio seconds. Throughput is turns/s for chat and
samples/s for speech. Criterion measures elapsed time, not process CPU time.

Private speech and normalization modules are referenced directly by benchmark
targets rather than copied or made public. The imported modules retain their
production visibility; narrow harness allowances cover unused APIs and
differences caused by compiling their test modules without the standard test harness.

## HTTP load

`just bench-e2e` builds both release binaries and starts a fresh server for every
load point. Authentication is off, stream delay is zero, connections are reused,
and request bodies are encoded once. Each point is warmed separately. Server
processes are killed and reaped after completion, errors, and Ctrl-C. Completed
point artifacts remain available after an interrupted run.
Server exits abort the run and leave `failure.json`, `server.log`, and partial
request samples in the failed point directory.

The binaries are built in separate Cargo invocations so benchmark dependency
features do not change the server build.

The default workloads are short unary chat, 100-user-turn unary chat, short SSE
chat, medium raw PCM speech, and a deterministic mix of nine short chat requests
followed by one medium speech request. The long fixture has 199 provider messages
and fits the default text bound. OpenAI speech always streams: raw audio and SSE
are different transports, not unary and streaming synthesis modes.

Closed-model points use 1/2/4/8/16/32/64 concurrent clients. Open-model points
offer 10/50/100/250/500 requests per second on an independent clock. Every point
runs three times, reversing point order on the second repetition. Change the
matrix with CLI arguments:

```sh
just bench-e2e --workloads chat-short,speech-medium-pcm --concurrency 1,8 --rates 10,100 --repeats 3
just bench-e2e --duration 60 --warmup 10 --server-cpus 0,1,2,3 --client-cpus 16,17,18,19
```

Supported chat names are `chat-short`, `chat-long`, `chat-sse`, and
`chat-long-sse`. Speech names are `speech-{short,medium,long}-{pcm,wav,mp3}`;
append `-sse` for base64 audio events. `mixed` selects the fixed ratio. Smoke
mode covers every transport/format/text-size combination and all four provider
families. Run `target/release/eliza-bench --help` for configuration options.

### Worker scaling

The server supports `--runtime-workers` and `--speech-workers`. The benchmark
accepts comma-separated lists and compares their Cartesian product on the same
CPU sets. Runtime workers default to the number of selected server CPUs;
speech workers default to two. Counts are deduplicated and sorted, then both
worker and load-point order reverse on alternate repetitions.

```sh
just bench-e2e --workloads chat-short,speech-medium-pcm,mixed \
  --runtime-workers 1,2,4,8 --speech-workers 1,2,4,8 \
  --server-cpus 16,17,18,19,20,21,22,23 --client-cpus 24,25,26,27,28,29,30,31 \
  --concurrency 1,8,32 --rates 10,100 --duration 60 --warmup 10 --repeats 3 \
  --probe-seconds 0 --output target/benchmarks/worker-scaling
```

This example reserves disjoint E-core sets on the local i9-14900K; choose CPU
sets for your host. Sixteen worker configurations multiply run time by sixteen.
Each configuration has its own metadata and results directory. Every result
records worker counts, throughput, CPU per successful request, p95/p99 latency,
sampled peak RSS, failures, and dropped arrivals. `by_workload` reports separate
latency distributions and counts for chat and speech in mixed runs; these
distributions include failed exchanges, as does the aggregate distribution.
Raw samples retain errors for separate failure analysis. Process CPU and RSS
describe the whole server, not individual modalities.

Compare each repetition separately. Select a worker limit using throughput,
CPU/request, chat latency during speech load, and memory together. Larger limits
retain more speech buffers and can increase total CPU consumption. No new
default or capacity claim follows from fixture smoke tests. Confirm results on
stable hardware before adopting a host-specific setting.

Validation on 2026-10-07 passed `just ok`, including 76 generation fixture cases,
seven HTTP contract tests, Clippy, and Dylint. Release validation exercised all
sixteen 1/2/4/8 worker combinations over short chat, medium PCM speech, and mixed
traffic, with concurrency eight and open ten requests/s in two alternating
repetitions. All 192 one-second points passed without failures or drops; separate
histogram counts matched recorded exchanges. Default release smoke also passed
46 points covering every speech format and streaming transport. These brief
checks validate execution, not capacity. Both runs used disjoint E-core sets
16–23 for the server and 24–31 for the client. Artifacts remain under
`target/benchmarks/worker-matrix-validation/` and
`target/benchmarks/worker-default-smoke/`.

Open-model completion latency starts at the intended arrival, including dispatch
delay. There is no catch-up burst: a missed arrival interval or occupied client
limit produces a recorded drop. The limit is rate times the ten-second request
deadline. Drops remain unsent work; neither failed nor dropped requests count as
successful throughput. High dispatch delay, drops, and load-generator CPU can
identify client saturation. Closed-model latency starts when a client offers its
next request; closed-model results can conceal overload and must be read beside
the open-model sweep.

Independent arrivals use one blocking clock worker with OS sleeps and absolute
deadlines. Only one tick can wait for the client executor. Queue-full, late, and
occupancy misses remain in the `dropped` counter; normal OS scheduling can still
miss deadlines. Client CPU includes this worker. The former Tokio sleep clock
lost most arrivals around 1,000 requests/s because its millisecond granularity
conflicted with the arrival period; see [the capacity report](PUBLIC_CAPACITY.md)
for the correction and retained failed measurements.

High-concurrency runs need enough file descriptors in both processes. Set
`ulimit -Sn 16384` in the launching shell for the 2,048-request sweep used in that
report. The client and its server inherit the limit, captured in `metadata.json`.

Before the sweep, separate one-request-per-second probes measure chat SSE, raw
PCM, and speech SSE for sixty seconds. They consume complete bodies and detect
the first nonempty content delta/audio payload, excluding WAV headers. Header
arrival is never labeled first generated content. Probes run without concurrent
load, so their latency does not describe loaded streaming behavior; request
samples in the load sweeps also record first-payload timing.
Use `--probe-seconds 0` to skip these probes for diagnostic runs, or set their
duration explicitly. Provider fixture checks still run before the first point.

## Results and interpretation

Each run writes to a fresh `target/benchmarks/<timestamp>/` directory:

- `metadata.json`: commit/diff, workspace manifest (including release profile),
  toolchain, lockfile, fixtures, CPU topology and frequency policy, memory,
  cgroup information, process limits, affinity, and runtime settings.
- `results.json`: independent per-point results, written after every point.
- Per-point `requests.jsonl`, `process.json`, `summary.json`, and `server.log`,
  plus separate warmup request samples and streaming probe artifacts.

Raw request samples record validation/transport errors, complete response size,
dispatch delay, first-payload latency, and completion latency. Response bodies
are bounded to 8 MiB. HDR histograms use microseconds and three significant
digits; reports include p50/p95/p99 and bucket counts. Do not average percentiles
or pool unrelated workloads.

`successful_rps` counts successes completed inside the offered-load window.
`successful_rps_including_drain` includes requests completed during the final
drain. CPU includes that drain and failed requests; CPU/request divides total
server user/system CPU seconds by all successful completions. Reports retain
errors and drops beside that ratio. CPU counters cover all server threads and
have the OS clock-tick resolution; short smoke windows are not CPU baselines.
`peak_concurrency` counts pending client tasks, including completed tasks awaiting
collection. Process CPU/RSS snapshots are sampled every 200 ms; RSS is a sampled
peak, not a guaranteed instantaneous maximum.

The declared capacity budgets are p95 <=100 ms for chat and <=5 seconds for
speech/mixed workloads, with zero failures and zero drops. Capacity requires all
three repetitions of a rate to pass. If the highest tested rate passes, capacity
is only a lower bound; extend the sweep before claiming saturation. Speech has
two worker permits, so adding clients can increase queue latency without adding
synthesis parallelism.

The automated runner requires Linux `/proc` and CPU affinity. It divides the
current logical CPU allowance between server and client by default. On hybrid
CPUs or SMT systems, choose explicit masks and inspect recorded sibling groups.
Loopback measurements still share memory, cache, and kernel resources; check
generator saturation before attributing a limit to Eliza. This runner does not
claim remote-host or platform-independent capacity. Criterion generation targets
can run on the repository's other supported platforms; manual HTTP measurements
there require recording CPU/RSS externally.

Compare commits with the same compiler, profile, fixtures, CPU masks, power
policy, and server settings. Alternate candidate/baseline run order and keep raw
artifacts. Treat changes smaller than repeated-run noise as inconclusive. Profile
separately from timed runs; shared CI timing should never gate a performance PR.
Hosted runs accept `--hosted-config PATH`, `--supporter-percent 10` and `--public-identities 100`. Supply `ELIZA_INGRESS_SECRET`, `ELIZA_ENTITLEMENT_SECRET` and, for supporter traffic, `ELIZA_BENCH_SUPPORTER_KEY` through the environment. Run the restricted Nitro billing endpoint with a confirmed test account/key. Credentials are omitted from samples and metadata. The policy is recorded; it must contain no secrets.

Use `image`, `embedding`, and `inline-image` alongside the existing workloads to exercise media admission. `--read-delay-ms 50` paces response consumption; `--upload-delay-ms 50` sends 1 KiB input chunks with delays. Keep the client request deadline in mind when interpreting deliberate slow-client failures. `--public-identities 131072` exercises bounded public identity storage; this is a synthetic verified-ingress case, not a bypass for real clients.

For comparisons, build each clean origin revision separately and copy its executable before another worktree overwrites a shared target directory. Use `--server-binary /absolute/path/eliza --server-commit FULL_SHA` to run the same driver against each origin. Metadata distinguishes the driver revision from the origin revision. Keep compiler, CPU affinity, workload, policy, warmup and measurement windows identical; use at least three repetitions and alternate baseline/candidate order. A local run on the development i9-14900K is evidence about that machine, not production capacity.

Native 429 and 503 responses with retry headers are reported as personal and shared rejections. Invalid envelopes and failed transports remain failures. Per-tier latency includes successful complete responses, while status counts and raw samples retain rejections. Bandwidth reports received bodies, including native rejection bodies and partial failed exchanges. CPU per success becomes misleading under heavy rejection; inspect total CPU, offered/accepted rates and rejection counts together.

`benches/hosted-stress.json` raises personal allowances solely to isolate global saturation and protected capacity. Its shared CPU, media, response, buffering and deadline budgets match staging. Run the exact staging policy separately to verify personal limits. Never deploy the stress profile as public policy.
