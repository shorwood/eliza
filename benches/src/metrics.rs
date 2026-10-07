//! Latency histograms and Linux process snapshots for one measurement window.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::Result;
use hdrhistogram::Histogram;
#[cfg(target_os = "linux")]
use nix::unistd::{SysconfVar, sysconf};
use serde::Serialize;
use serde_json::{Value, json};

use crate::workload::Sample;

// -----------------------------------------------------------------------------
// Measurements: Raw samples and latency histograms for one load point.
// -----------------------------------------------------------------------------

/// Raw samples and latency histograms for one load point.
pub struct Measurements {
    /// Buffered raw request samples.
    raw: BufWriter<File>,
    /// Completion latency including scheduling delay.
    completion: Histogram<u64>,
    /// Latency to the first meaningful payload.
    first: Histogram<u64>,
    /// Delay between scheduled arrival and request dispatch.
    dispatch: Histogram<u64>,
    /// Separate completion distributions for each workload in a mixed run.
    by_workload: BTreeMap<String, Histogram<u64>>,
    /// Native personal allowance rejections.
    personal_rejections: u32,
    /// Native shared-capacity rejections.
    shared_rejections: u32,
    /// Request counts by tier and status, including transport failures.
    by_status: BTreeMap<String, u32>,
    /// Received validated payload bytes.
    bytes: usize,
    /// Completion distributions separated by caller tier.
    by_tier: BTreeMap<String, Histogram<u64>>,
    /// Successful complete requests including drain.
    pub successful: u32,
    /// Requests that failed transport or response validation.
    failures: u32,
    /// Successful requests completed before the measurement deadline.
    pub completed_in_window: u32,
}

impl Measurements {
    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    pub fn new(path: &Path) -> Result<Self> {
        Ok(Self {
            raw: BufWriter::new(File::create(path)?),
            completion: Histogram::new_with_bounds(1, 3_600_000_000, 3)?,
            first: Histogram::new_with_bounds(1, 3_600_000_000, 3)?,
            dispatch: Histogram::new_with_bounds(1, 3_600_000_000, 3)?,
            by_workload: BTreeMap::new(),
            personal_rejections: 0,
            shared_rejections: 0,
            bytes: 0,
            by_status: BTreeMap::new(),
            by_tier: BTreeMap::new(),
            successful: 0,
            failures: 0,
            completed_in_window: 0,
        })
    }

    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    pub fn record(&mut self, sample: &Sample, end: tokio::time::Instant) -> Result<()> {
        serde_json::to_writer(&mut self.raw, sample)?;
        self.raw.write_all(b"\n")?;
        self.completion.record(sample.completion_us)?;
        let histogram = match self.by_workload.entry(sample.workload.clone()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Histogram::new_with_bounds(1, 3_600_000_000, 3)?),
        };
        histogram.record(sample.completion_us)?;
        let tier = match self.by_tier.entry(sample.tier.into()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(Histogram::new_with_bounds(1, 3_600_000_000, 3)?),
        };
        if sample.error.is_none() && sample.status == Some(reqwest::StatusCode::OK.as_u16()) {
            tier.record(sample.completion_us)?;
        }
        let status = sample
            .status
            .map_or_else(|| "transport".into(), |code| code.to_string());
        *self
            .by_status
            .entry(format!("{}/{status}", sample.tier))
            .or_default() += 1;
        self.bytes += sample.bytes;
        self.personal_rejections += u32::from(sample.status == Some(429) && sample.error.is_none());
        self.shared_rejections += u32::from(sample.status == Some(503) && sample.error.is_none());
        self.dispatch.record(sample.dispatch_us)?;
        if sample.error.is_none() && sample.status == Some(reqwest::StatusCode::OK.as_u16()) {
            self.successful += 1;
            self.completed_in_window += u32::from(sample.finished_at < end);
            if let Some(first) = sample.first_us {
                self.first.record(first)?;
            }
        } else if sample.error.is_some() || !matches!(sample.status, Some(429 | 503)) {
            self.failures += 1;
        }
        Ok(())
    }

    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    pub fn summary(&mut self) -> Result<Value> {
        self.raw.flush()?;
        let by_workload = self.by_workload.iter().map(|(name, histogram)| {
            (name, json!({"requests":histogram.len(), "completion_ms":quantiles(histogram), "completion_histogram_us":bins(histogram)}))
        }).collect::<BTreeMap<_, _>>();
        let by_tier = self
            .by_tier
            .iter()
            .map(|(name, histogram)| (name, quantiles(histogram)))
            .collect::<BTreeMap<_, _>>();
        Ok(
            json!({"successful":self.successful, "failures":self.failures,
            "personal_rejections":self.personal_rejections, "shared_rejections":self.shared_rejections,
            "received_bytes":self.bytes, "by_tier":by_tier, "by_status":self.by_status,
            "completed_in_window":self.completed_in_window,
            "completion_ms":quantiles(&self.completion), "first_ms":quantiles(&self.first),
            "dispatch_ms":quantiles(&self.dispatch),
            "by_workload":by_workload,
            "completion_histogram_us":bins(&self.completion), "first_histogram_us":bins(&self.first)}),
        )
    }
}

// -----------------------------------------------------------------------------
// Quantiles: Report quantiles without averaging unrelated samples.
// -----------------------------------------------------------------------------

/// Report quantiles without averaging unrelated samples.
fn quantiles(histogram: &Histogram<u64>) -> Value {
    // Empty measurements have no latency distribution.
    if histogram.is_empty() {
        return Value::Null;
    }
    let milliseconds = |quantile| {
        std::time::Duration::from_micros(histogram.value_at_quantile(quantile)).as_secs_f64()
            * 1000.0
    };
    json!({"p50":milliseconds(0.5), "p95":milliseconds(0.95), "p99":milliseconds(0.99)})
}

// -----------------------------------------------------------------------------
// Bins: Export the histogram buckets for reproducible analysis.
// -----------------------------------------------------------------------------

/// Export the histogram buckets for reproducible analysis.
fn bins(histogram: &Histogram<u64>) -> Value {
    let bins = histogram.iter_recorded().map(|bin| json!({"upper_us":bin.value_iterated_to(), "count":bin.count_since_last_iteration()})).collect::<Vec<_>>();
    json!(bins)
}

// -----------------------------------------------------------------------------
// Process: CPU and memory snapshot for a complete process.
// -----------------------------------------------------------------------------

/// CPU and memory snapshot for a complete process.
#[derive(Clone, Copy, Serialize)]
pub struct Process {
    /// Accumulated user CPU time for all process threads.
    pub user_seconds: f64,
    /// Accumulated kernel CPU time for all process threads.
    pub system_seconds: f64,
    /// Current resident memory in bytes.
    pub rss_bytes: u64,
}

impl Process {
    /// Read Linux process-wide CPU and resident memory counters.
    ///
    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    #[cfg(target_os = "linux")]
    pub fn read(pid: u32) -> Result<Self> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
        let fields = stat
            .rsplit_once(')')
            .ok_or_else(|| anyhow::anyhow!("invalid process stat"))?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        let ticks = f64::from(u32::try_from(
            sysconf(SysconfVar::CLK_TCK)?.ok_or_else(|| anyhow::anyhow!("no clock tick value"))?,
        )?);
        let pages = u64::try_from(
            sysconf(SysconfVar::PAGE_SIZE)?.ok_or_else(|| anyhow::anyhow!("no page size"))?,
        )?;
        Ok(Self {
            user_seconds: fields[11].parse::<f64>()? / ticks,
            system_seconds: fields[12].parse::<f64>()? / ticks,
            rss_bytes: fields[21].parse::<u64>()? * pages,
        })
    }

    /// Read Linux process-wide CPU and resident memory counters.
    ///
    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    #[cfg(not(target_os = "linux"))]
    pub fn read(_: u32) -> Result<Self> {
        anyhow::bail!("automatic process measurements require Linux")
    }
}

// -----------------------------------------------------------------------------
// Save: Write one reproducible JSON artifact.
// -----------------------------------------------------------------------------

/// Write one reproducible JSON artifact.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
#[allow(
    rlib::foreign_type_method_like_free_functions,
    reason = "one artifact writer does not require a Path wrapper"
)]
pub fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut output = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()?;
    Ok(())
}
