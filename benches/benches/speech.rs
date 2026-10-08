//! Separate benchmark crate using the production speech modules.
#![feature(register_tool)]
#![register_tool(rlib)]

// Imported production modules retain their public API and declaration order.
// Their unused APIs and test-only lint expectations do not apply to this harness.
#[allow(
    dead_code,
    unused_imports,
    unfulfilled_lint_expectations,
    rlib::unnecessarily_broad_visibility,
    rlib::misordered_module_declarations
)]
#[path = "../../crates/modality-speech/src/audio.rs"]
mod audio;
#[allow(
    dead_code,
    unused_imports,
    rlib::unnecessarily_broad_visibility,
    rlib::misordered_module_declarations
)]
#[path = "../../crates/modality-speech/src/core.rs"]
mod core;
#[allow(
    dead_code,
    unused_imports,
    rlib::unnecessarily_broad_visibility,
    rlib::misordered_module_declarations
)]
#[path = "../../crates/modality-speech/src/errors.rs"]
mod errors;
#[allow(
    dead_code,
    unused_imports,
    rlib::unnecessarily_broad_visibility,
    rlib::misordered_module_declarations
)]
#[path = "../../crates/modality-speech/src/service.rs"]
mod service;
#[allow(
    dead_code,
    unused_imports,
    rlib::unnecessarily_broad_visibility,
    rlib::misordered_module_declarations
)]
#[path = "../../crates/modality-speech/src/synth.rs"]
mod synth;

mod support;

use std::hint::black_box;
use std::num::NonZeroUsize;

use criterion::{
    BatchSize, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};

use crate::core::{AudioFormat, Request, Segment};

// -----------------------------------------------------------------------------
// Fixtures: Fixed speech text shared with HTTP measurements.
// -----------------------------------------------------------------------------

/// Baseline speech inputs.
const FIXTURES: &str = include_str!("../fixtures.json");

// -----------------------------------------------------------------------------
// Request: Fixed voice, speed, and output rate.
// -----------------------------------------------------------------------------

/// Prepare one speech request without changing production synthesis setup.
fn request(text: &str, sample_rate: u32) -> Request {
    Request {
        segments: vec![Segment {
            text: text.to_owned(),
            voice: "Kore".to_owned(),
            style: String::new(),
            speed: 1.0,
            pause_after_ms: 0,
        }],
        sample_rate,
    }
}

// -----------------------------------------------------------------------------
// Speech: Synthesis and service workload matrix.
// -----------------------------------------------------------------------------

/// Measure synthesis, encoding, and the public service independently.
///
/// # Panics
/// Panics when speech fixtures, sample counts, or encoded formats regress.
fn speech(criterion: &mut Criterion) {
    let fixtures: serde_json::Value = serde_json::from_str(FIXTURES).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut group = criterion.benchmark_group("speech");
    group.sampling_mode(SamplingMode::Flat);
    for name in ["short", "medium", "long"] {
        let text = fixtures["speech"][name].as_str().unwrap();
        for rate in [8_000, 24_000] {
            let request = request(text, rate);
            let samples = synth::synthesize(&request, rate as usize * 120).unwrap();
            assert!(!samples.is_empty());
            assert!(samples.iter().any(|sample| *sample != 0));
            eprintln!(
                "speech/{name}/{rate}: {} samples, {:.6} audio seconds",
                samples.len(),
                f64::from(u32::try_from(samples.len()).unwrap()) / f64::from(rate)
            );
            group.throughput(Throughput::Elements(u64::try_from(samples.len()).unwrap()));
            group.bench_function(
                BenchmarkId::new("synthesis", format!("{name}/{rate}")),
                |bench| {
                    bench.iter(|| {
                        black_box(
                            synth::synthesize(black_box(&request), rate as usize * 120).unwrap(),
                        )
                    });
                },
            );
            encode_and_service(&mut group, &runtime, &request, &samples, name);
        }
    }
    styles(&mut group, &fixtures);
    group.finish();
}

// -----------------------------------------------------------------------------
// Styles: Additional fixed voice controls and segment cases.
// -----------------------------------------------------------------------------

/// Measure styled and multi-segment synthesis independently.
///
/// # Panics
/// Panics if a fixed synthesis fixture fails validation.
fn styles(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    fixtures: &serde_json::Value,
) {
    let limit = NonZeroUsize::new(support::BENCHMARK_MAX_INPUT_CHARS).unwrap();
    for name in ["style", "segments"] {
        let mut request = request(fixtures["speech"]["short"].as_str().unwrap(), 24_000);
        if name == "style" {
            "calm and low".clone_into(&mut request.segments[0].style);
        } else {
            request.segments.push(request.segments[0].clone());
        }
        request.validate(limit).unwrap();
        let samples = synth::synthesize(&request, 24_000 * 120).unwrap();
        group.throughput(Throughput::Elements(u64::try_from(samples.len()).unwrap()));
        group.bench_function(BenchmarkId::new("synthesis", name), |bench| {
            bench.iter(|| black_box(synth::synthesize(black_box(&request), 24_000 * 120).unwrap()));
        });
    }
}

// -----------------------------------------------------------------------------
// EncodeAndService: Separate encoder and public-service measurements.
// -----------------------------------------------------------------------------

/// Benchmark encoders on precomputed PCM and the complete speech service.
///
/// # Panics
/// Panics when encoding, service rendering, or sample accounting regress.
fn encode_and_service(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    runtime: &tokio::runtime::Runtime,
    request: &Request,
    samples: &[i16],
    name: &str,
) {
    let rate = request.sample_rate;
    let service = service::Service::default();
    let limit = NonZeroUsize::new(support::BENCHMARK_MAX_INPUT_CHARS).unwrap();
    for (format_name, format) in [
        ("pcm", AudioFormat::Pcm),
        ("wav", AudioFormat::Wav),
        ("mp3", AudioFormat::Mp3),
    ] {
        let encoded = audio::EncodedAudio::encode(samples, rate, format).unwrap();
        assert!(!encoded.bytes.is_empty());
        assert_eq!(encoded.media_type, format.media_type(rate));
        if format == AudioFormat::Wav {
            assert_eq!(&encoded.bytes[..4], b"RIFF");
        }
        if format == AudioFormat::Pcm {
            assert_eq!(encoded.bytes.len(), samples.len() * 2);
        }
        group.bench_function(
            BenchmarkId::new("encode", format!("{name}/{rate}/{format_name}")),
            |bench| {
                bench.iter(|| {
                    black_box(
                        audio::EncodedAudio::encode(black_box(samples), rate, format).unwrap(),
                    )
                });
            },
        );
        let rendered = runtime
            .block_on(service.render(request.clone(), format, limit))
            .unwrap();
        assert_eq!(rendered.sample_count, samples.len());
        assert_eq!(rendered.sample_rate, rate);
        group.bench_function(
            BenchmarkId::new("service", format!("{name}/{rate}/{format_name}")),
            |bench| {
                bench.iter_batched(
                    || request.clone(),
                    |request| {
                        black_box(
                            runtime
                                .block_on(service.render(request, format, limit))
                                .unwrap(),
                        )
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().warm_up_time(support::BENCHMARK_WARMUP)
        .measurement_time(support::BENCHMARK_MEASUREMENT).sample_size(support::BENCHMARK_SAMPLES);
    targets = speech
}
criterion_main!(benches);
