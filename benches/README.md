# Rust HTTP load driver

This workspace crate lives outside production source. It carries the existing Rust load driver into the commercial PR stack; its generation workloads include chat, speech (PCM/WAV/MP3), streaming, and mixed traffic. The server remains a separate child process.

```sh
nix develop --command cargo build --release -p eliza-cli -p eliza-benches --locked
nix develop --command target/release/eliza-bench --smoke --server-cpus 16,17,18,19,20,21,22,23 --client-cpus 24,25,26,27,28,29,30,31
```

Use `--help` for workload, concurrency, offered-rate, warmup, duration, repeats, runtime workers and speech workers. Keep server and client CPU sets disjoint. Smoke checks exercise fixtures without timing assertions; measure full runs separately with warmup and repeated samples. Open-loop runs retain dropped arrivals rather than silently lowering offered load. Closed-loop runs measure behavior at fixed concurrency.

Artifacts include per-request timings, completion/first-byte percentiles, successful throughput, failures, dropped arrivals, server CPU, RSS and environment/revision metadata. Compare exact committed baseline and candidate revisions on the same machine and compiler. Do not infer hosted production capacity from earlier local reports or a smoke run.

Hosted mixed-tier flooding, rejection classification, slow-client cases and baseline/candidate evidence are added in PR 017B. Benchmarks remain Rust-only.
