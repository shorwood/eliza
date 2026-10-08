//! Fixed generation performance fixtures.
#![feature(register_tool)]
#![register_tool(rlib)]

mod support;

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use eliza_mad::engine::{Doctor, TraceSource};

/// Measure compilation, startup, and bounded generation.
///
/// # Panics
/// Panics when bundled fixtures or generation fail validation.
fn generation(criterion: &mut Criterion) {
    let doctor = Doctor::compile().unwrap();
    let mut group = criterion.benchmark_group("eliza");
    group.bench_function("compile", |bench| {
        bench.iter(|| black_box(Doctor::compile().unwrap()));
    });
    group.bench_function("session", |bench| {
        bench.iter(|| black_box(doctor.session().unwrap()));
    });
    for (name, input, source, keyword) in [
        ("keyword", "I NEED HELP", TraceSource::Keyword, "I"),
        ("fallback", "BANANAS", TraceSource::None, "NONE"),
        ("link", "MEN ARE ALL ALIKE", TraceSource::Keyword, "DIT"),
        ("pre", "I'M SAD", TraceSource::Keyword, "I"),
        ("newkey", "REMEMBER IF YOU CARE", TraceSource::None, "NONE"),
    ] {
        let mut session = doctor.session().unwrap();
        let expected = session.respond(input).unwrap();
        assert!(!expected.text.is_empty());
        assert_eq!(expected.trace.source, source, "{expected:?}");
        assert_eq!(expected.trace.selected_rule.unwrap().keyword, keyword);
        group.throughput(Throughput::Elements(1));
        group.bench_function(BenchmarkId::new("turn", name), |bench| {
            bench.iter_batched_ref(
                || doctor.session().unwrap(),
                |session| black_box(session.respond(black_box(input)).unwrap()),
                BatchSize::SmallInput,
            );
        });
    }
    for count in [1_u64, 10, 50, 100] {
        group.throughput(Throughput::Elements(count));
        group.bench_function(BenchmarkId::new("replay", count), |bench| {
            bench.iter(|| {
                let mut session = doctor.session().unwrap();
                for _ in 0..count {
                    black_box(session.respond(black_box("I NEED HELP")).unwrap());
                }
            });
        });
    }
    group.throughput(Throughput::Elements(1));
    let mut memory = doctor.session().unwrap();
    memory.respond("MY BOYFRIEND MADE ME COME HERE").unwrap();
    memory.respond("BANANAS").unwrap();
    assert_eq!(
        memory.respond("BULLIES").unwrap().trace.source,
        TraceSource::Memory
    );
    group.bench_function("memory", |bench| {
        bench.iter_batched_ref(
            || {
                let mut session = doctor.session().unwrap();
                session.respond("MY BOYFRIEND MADE ME COME HERE").unwrap();
                session.respond("BANANAS").unwrap();
                session
            },
            |session| black_box(session.respond("BULLIES").unwrap()),
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().warm_up_time(support::BENCHMARK_WARMUP)
        .measurement_time(support::BENCHMARK_MEASUREMENT).sample_size(support::BENCHMARK_SAMPLES);
    targets = generation
}
criterion_main!(benches);
