# Rust HTTP load driver

This workspace crate lives outside production source. It carries the existing Rust load driver into the commercial PR stack; its generation workloads include chat, speech (PCM/WAV/MP3), streaming, and mixed traffic. The server remains a separate child process.

```sh
nix develop --command cargo build --release -p eliza-cli -p eliza-benches --locked
nix develop --command target/release/eliza-bench --smoke --server-cpus 16,17,18,19,20,21,22,23 --client-cpus 24,25,26,27,28,29,30,31
```

Use `--help` for workload, concurrency, offered-rate, warmup, duration, repeats, runtime workers and speech workers. Keep server and client CPU sets disjoint. Smoke checks exercise fixtures without timing assertions; measure full runs separately with warmup and repeated samples. Open-loop runs retain dropped arrivals rather than silently lowering offered load. Closed-loop runs measure behavior at fixed concurrency.

Artifacts include per-request timings, completion/first-byte percentiles, successful throughput, failures, dropped arrivals, server CPU, RSS and environment/revision metadata. Compare exact committed baseline and candidate revisions on the same machine and compiler. Do not infer hosted production capacity from earlier local reports or a smoke run.

Benchmarks remain Rust-only.

Hosted runs accept `--hosted-config PATH`, `--supporter-percent 10` and `--public-identities 100`. Supply `ELIZA_INGRESS_SECRET`, `ELIZA_ENTITLEMENT_SECRET` and, for supporter traffic, `ELIZA_BENCH_SUPPORTER_KEY` through the environment. Run the restricted Nitro billing endpoint with a confirmed test account/key. Credentials are omitted from samples and metadata. The policy is recorded; it must contain no secrets.

Use `image`, `embedding`, and `inline-image` alongside the existing workloads to exercise media admission. `--read-delay-ms 50` paces response consumption; `--upload-delay-ms 50` sends 1 KiB input chunks with delays. Keep the client request deadline in mind when interpreting deliberate slow-client failures. `--public-identities 131072` exercises bounded public identity storage; this is a synthetic verified-ingress case, not a bypass for real clients.

For comparisons, build each clean origin revision separately and copy its executable before another worktree overwrites a shared target directory. Use `--server-binary /absolute/path/eliza --server-commit FULL_SHA` to run the same driver against each origin. Metadata distinguishes the driver revision from the origin revision. Keep compiler, CPU affinity, workload, policy, warmup and measurement windows identical; use at least three repetitions and alternate baseline/candidate order. A local run on the development i9-14900K is evidence about that machine, not production capacity.

Native 429 and 503 responses with retry headers are reported as personal and shared rejections. Invalid envelopes and failed transports remain failures. Per-tier latency includes successful complete responses, while status counts and raw samples retain rejections. Bandwidth reports received bodies, including native rejection bodies and partial failed exchanges. CPU per success becomes misleading under heavy rejection; inspect total CPU, offered/accepted rates and rejection counts together.

`benches/hosted-stress.json` raises personal allowances solely to isolate global saturation and protected capacity. Its shared CPU, media, response, buffering and deadline budgets match staging. Run the exact staging policy separately to verify personal limits. Never deploy the stress profile as public policy.
