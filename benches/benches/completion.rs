//! Fixed completion performance fixtures.
#![feature(register_tool)]
#![register_tool(rlib)]

// Measure the private production adapter without widening its API.
#[allow(dead_code, unused_imports, rlib::unnecessarily_broad_visibility)]
#[path = "../../crates/modality-chat/src/eliza.rs"]
mod eliza;

mod support;

use std::hint::black_box;
use std::num::NonZeroUsize;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use eliza_modality_chat::structured_output::StructuredOutput;
use eliza_modality_chat::turn::{Request, ToolChoice, Turn};

// -----------------------------------------------------------------------------
// TraceMode: Optional provider-visible diagnostics.
// -----------------------------------------------------------------------------

/// Requested trace rendering for this completion fixture.
#[derive(Clone, Copy)]
enum TraceMode {
    /// Ordinary output without rendered trace.
    Plain,
    /// Render and account for the mechanical trace.
    Included,
}

// -----------------------------------------------------------------------------
// Request: Fresh bounded history for one completion.
// -----------------------------------------------------------------------------

/// Build a fresh history so every measured iteration has identical state.
fn request(count: usize, input: &str, trace: TraceMode) -> Request {
    let mut turns = Vec::new();
    for index in 0..count {
        turns.push(Turn::from(input.to_owned()));
        if index + 1 >= count {
            continue;
        }
        turns.push(Turn::Assistant("OK".to_owned()));
    }
    let request = Request::new(
        Vec::new(),
        turns,
        Vec::new(),
        ToolChoice::Auto,
        StructuredOutput::default(),
    );
    match trace {
        TraceMode::Plain => request,
        TraceMode::Included => request.with_reasoning(),
    }
}

// -----------------------------------------------------------------------------
// NormalizationInput: Text and its exact terminal record.
// -----------------------------------------------------------------------------

/// One input-adaptation fixture shared by helper and completion measurements.
struct NormalizationInput {
    /// Stable benchmark case name.
    name: &'static str,
    /// Unmodified provider input.
    input: String,
    /// Expected historical terminal record.
    expected: String,
}

// -----------------------------------------------------------------------------
// Completion: Neutral replay and accounting workloads.
// -----------------------------------------------------------------------------

/// Long inputs exercise scanning before and after the retained record.
fn completion_long_inputs() -> [NormalizationInput; 2] {
    let record = format!("I NEED HELP {}", "A".repeat(60));
    [
        NormalizationInput {
            name: "long_prefix",
            input: format!("{}I need help", "\u{2003}".repeat(4096)),
            expected: "I NEED HELP".to_owned(),
        },
        NormalizationInput {
            name: "long_suffix",
            input: format!("{record}{}", " —".repeat(2048)),
            expected: record,
        },
    ]
}

/// Measure the private input adapter with fixture creation outside timing.
///
/// # Panics
/// Panics when the fixed normalized records change.
fn completion_normalization(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("normalization");
    let boundary = "a".repeat(71);
    let mut inputs = vec![
        NormalizationInput {
            name: "ascii",
            input: "I need help".to_owned(),
            expected: "I NEED HELP".to_owned(),
        },
        NormalizationInput {
            name: "unicode",
            input: "I’m sad—really!".to_owned(),
            expected: "I'M SAD REALLY".to_owned(),
        },
        NormalizationInput {
            name: "boundary",
            input: format!("{boundary} b"),
            expected: format!("{} ", boundary.to_ascii_uppercase()),
        },
        NormalizationInput {
            name: "discarded",
            input: "—".repeat(4096),
            expected: String::new(),
        },
    ];
    inputs.extend(completion_long_inputs());
    for NormalizationInput {
        name,
        input,
        expected,
    } in inputs
    {
        assert_eq!(eliza::input_record(&input), expected);
        group.bench_function(name, |bench| {
            bench.iter(|| black_box(eliza::input_record(black_box(&input))));
        });
    }
    group.finish();
}

/// Measure provider-neutral completion with fixed histories.
///
/// # Panics
/// Panics when fixtures cannot complete or usage/trace accounting changes.
fn completion(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("completion");
    let chars = NonZeroUsize::new(support::BENCHMARK_MAX_INPUT_CHARS).unwrap();
    let messages = NonZeroUsize::new(support::BENCHMARK_MAX_HISTORY).unwrap();
    for count in [1, 10, 50, 100] {
        for (name, input, reasoning) in [
            ("ascii", "I need help", TraceMode::Plain),
            ("unicode", "I’m sad—really!", TraceMode::Plain),
            ("reasoning", "I need help", TraceMode::Included),
        ] {
            let response = request(count, input, reasoning)
                .complete(chars, messages)
                .unwrap();
            assert!(response.usage.completion > 0);
            assert_eq!(
                response.reasoning.is_some(),
                matches!(reasoning, TraceMode::Included)
            );
            group.throughput(Throughput::Elements(u64::try_from(count).unwrap()));
            group.bench_function(BenchmarkId::new(name, count), |bench| {
                bench.iter_batched(
                    || request(count, input, reasoning),
                    |request| black_box(request.complete(chars, messages).unwrap()),
                    BatchSize::SmallInput,
                );
            });
        }
    }
    for NormalizationInput {
        name,
        input,
        expected,
    } in completion_long_inputs()
    {
        assert_eq!(eliza::input_record(&input), expected);
        let response = request(1, &input, TraceMode::Plain)
            .complete(chars, messages)
            .unwrap();
        let reference = request(1, &expected, TraceMode::Plain)
            .complete(chars, messages)
            .unwrap();
        assert_eq!(response.output, reference.output);
        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new(name, 1), |bench| {
            bench.iter_batched(
                || request(1, &input, TraceMode::Plain),
                |request| black_box(request.complete(chars, messages).unwrap()),
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().warm_up_time(support::BENCHMARK_WARMUP)
        .measurement_time(support::BENCHMARK_MEASUREMENT).sample_size(support::BENCHMARK_SAMPLES);
    targets = completion_normalization, completion
}
criterion_main!(benches);
