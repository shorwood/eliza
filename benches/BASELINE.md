# Local baseline — 2026-10-07

Generation measurements are complete. The full HTTP baseline is incomplete:
the server aborted at two concurrent short-chat clients with
`double free or corruption (out)`. A separate attempt reproduced that failure.
No HTTP capacity bound can be established from these runs.

The server has no optimization changes from PRs 001–004. The implementation is
based on commit `61ffe52b98f5928fe3d60298f4e75997530a92ba` plus PR 005's benchmark
crate, using Rust `1.97.0-nightly` (`e8e4541ff`) and LLVM 22.1.2. Release profile
settings remain Cargo defaults. The local host is an Intel Core i9-14900K.

## Generation

Criterion used 50 samples, three-second warmup, and ten-second measurement per
case. Values below are mean estimates with 95% bootstrap confidence intervals;
they are elapsed time, not CPU time. Generation was run separately from HTTP
load. See [timing boundaries](README.md#generation) before comparing results.

| Operation | Mean | 95% interval |
| --- | ---: | ---: |
| Compile DOCTOR | 720.2 µs | 717.1–723.9 µs |
| Start and drop a session | 68.82 µs | 68.59–69.08 µs |
| Isolated keyword turn | 7.724 µs | 7.537–7.909 µs |
| Start a session and replay 100 turns | 717.3 µs | 715.8–718.8 µs |
| Neutral ASCII completion, one user turn | 72.16 µs | 71.72–72.61 µs |
| Neutral ASCII completion, 100 user turns | 751.7 µs | 749.0–754.9 µs |
| Neutral completion with rendered reasoning, one turn | 86.36 µs | 84.25–89.85 µs |
| Flite synthesis, short text, 24 kHz | 0.9634 ms | 0.9616–0.9656 ms |
| Flite synthesis, medium text, 24 kHz | 6.455 ms | 6.441–6.470 ms |
| Flite synthesis, long text, 24 kHz | 19.19 ms | 19.13–19.25 ms |
| PCM encoding, medium precomputed audio, 24 kHz | 6.359 µs | 6.345–6.374 µs |
| WAV encoding, medium precomputed audio, 24 kHz | 13.35 µs | 13.28–13.46 µs |
| MP3 encoding, medium precomputed audio, 24 kHz | 16.87 ms | 16.83–16.93 ms |
| Complete PCM speech service, medium text, 24 kHz | 6.878 ms | 6.604–7.228 ms |
| Complete MP3 speech service, long text, 24 kHz | 71.29 ms | 71.12–71.49 ms |

Session initialization is larger than isolated generation for this short chat
fixture. MP3 encoding is larger than synthesis for the medium speech fixture.
These results support measuring session sharing and encoding costs separately;
they do not predict the speedup of an unimplemented optimization.

Final estimates are in `target/criterion/**/before/estimates.json`. The first
engine run was interrupted to exclude session teardown from isolated turns.
Automatic comparison lines in `engine-baseline.log` can therefore describe a
timing-boundary correction rather than a code optimization; the table uses only
the final saved estimates.

The fixed speech corpus produces the following durations at 24 kHz. Real-time
factor uses the raw synthesis mean above; lower is better.

| Text | Samples | Audio seconds | Real-time factor |
| --- | ---: | ---: | ---: |
| Short | 34,380 | 1.432500 | 0.000673 |
| Medium | 245,358 | 10.223250 | 0.000631 |
| Long | 730,035 | 30.418125 | 0.000631 |

## HTTP results and methodology

The full sweep completed its streaming probes and the first short-chat point
before failing at `chat-short Closed 2 r1`. The completed one-client point
measured 1,463 successful requests/s, p95 0.723 ms, approximately 639 µs of
server CPU per successful request, and 150.7 MiB sampled peak RSS. These are
one-run observations, not an independently repeated baseline.

A diagnostic run with the default CPU allocation reproduced the same abort
at two concurrent short-chat clients. Its `failure.json` records server exit
status 134 and its `server.log` contains the allocator error. The harness stops
on server exit, preserves partial request samples, and reaps the child process.

A second diagnostic used CPUs 16–23 for the server and 24–31 for the client.
Three 15-second concurrency-two points and three 15-second rate-ten points
completed without failures or drops. Compilation overlapped part of this
diagnostic, so its timings are not baseline measurements. Changing allocation
is evidence to investigate, not proof of the crash's cause. The local pinned
compiler also crashed intermittently during earlier builds; no causal link
between those crashes and the server abort has been established.

Resolve the server abort or use a verified stable runner before completing
the HTTP capacity baseline. The following describes the attempted full matrix.

The full matrix uses five workloads, seven concurrency levels, five offered
rates, and three repetitions. Each point has ten-second warmup and sixty-second
measurement, with a fresh server and reused connections. Point order reverses
on the second repetition. Separate sixty-second streaming probes run first.

The server uses logical CPUs 0–15 and sixteen Tokio workers; the client uses
logical CPUs 16–31. The masks do not overlap. On this hybrid CPU, the server and
client use different core classes; loopback still shares cache, memory, and the
kernel. Results characterize this allocation, not remote-host capacity.
The host runs NixOS/Linux 6.18.47 with approximately 94 GiB RAM and the
`performance` CPU governor. Root `cpu.max` was unavailable; the saved cgroup
information identifies the process context, but this run has no verified CPU
quota value.

The acceptance budgets are p95 <=100 ms for chat and <=5 seconds for speech and
mixed requests, with no failures or drops. A rate must pass all three repetitions.
An unbracketed ceiling will be reported as a lower bound, never a maximum.

Raw results, process snapshots, fixtures, compiler details, and machine metadata
are retained under `target/benchmarks/baseline-2026-10-07/`. Diagnostic artifacts
are in `target/benchmarks/diagnostic-default/` and
`target/benchmarks/diagnostic-ecores/`. Reproduce with:

```sh
just bench-generation --save-baseline before
just bench-e2e --output target/benchmarks/new-baseline
```

The focused reproduction skips streaming probes:

```sh
just bench-e2e --workloads chat-short --concurrency 2 --rates 10 --duration 60 --warmup 2 --repeats 3 --probe-seconds 0
```

## Validation

`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 just ok` passed, including all workspace
tests, HTTP contracts, generation smoke cases, Clippy, and Dylint. This was rerun
after the timing and server-exit reporting changes. `just bench-smoke`
passed all 68 generation cases and 46 HTTP load points across every supported
benchmark transport/format/text-size combination.
