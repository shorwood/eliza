//! Shared measurement settings and server fixture limits.

use std::time::Duration;

// -----------------------------------------------------------------------------
// Benchmark: Sampling settings and accepted fixture limits.
// -----------------------------------------------------------------------------

/// Warm caches before collecting measurements.
pub(super) const BENCHMARK_WARMUP: Duration = Duration::from_secs(3);

/// Measurement interval for each statistical sample group.
pub(super) const BENCHMARK_MEASUREMENT: Duration = Duration::from_secs(10);

/// Samples per comparison, keeping synthesis runs bounded.
pub(super) const BENCHMARK_SAMPLES: usize = 50;

/// Default server request text bound.
#[allow(
    dead_code,
    rlib::unnecessarily_broad_visibility,
    reason = "shared limits are used by only some benchmark targets"
)]
pub(super) const BENCHMARK_MAX_INPUT_CHARS: usize = 8_000;

/// Default server history bound.
#[allow(
    dead_code,
    rlib::unnecessarily_broad_visibility,
    reason = "shared limits are used by only some benchmark targets"
)]
pub(super) const BENCHMARK_MAX_HISTORY: usize = 200;
