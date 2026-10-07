//! Fixed-concurrency and independent-arrival scheduling.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::future::{Either, select};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, sleep_until};

use crate::metrics::Measurements;
use crate::workload::{Sample, Workloads};

// -----------------------------------------------------------------------------
// Mode: Scheduling policy for a load point.
// -----------------------------------------------------------------------------

/// Scheduling model for one load point.
#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    /// Clients replace completed requests immediately.
    Closed,
    /// Arrivals occur independently of request completion.
    Open,
}

// -----------------------------------------------------------------------------
// Point: One fixed load level.
// -----------------------------------------------------------------------------

/// One fixed load level.
#[derive(Clone, Copy)]
pub(super) struct Point {
    /// Concurrency or arrival-rate scheduling.
    pub mode: Mode,
    /// Concurrent clients or offered requests per second.
    pub amount: u32,
    /// Time during which clients offer work.
    pub seconds: u32,
}

// -----------------------------------------------------------------------------
// Pacer: Publishes independent arrivals from one absolute-deadline clock.
// -----------------------------------------------------------------------------

/// Original sequence and deadline of an independently scheduled arrival.
struct PacerArrival {
    /// Global fixture sequence, including missed arrivals.
    sequence: u32,
    /// Absolute dispatch deadline.
    scheduled: Instant,
}

/// One queued tick prevents delayed consumers from receiving a catch-up burst.
const PACER_PENDING_ARRIVALS: usize = 1;

/// Bounded arrival clock using OS sleeps rather than millisecond Tokio timers.
struct Pacer {
    /// Arrival waiting for the client executor.
    arrivals: mpsc::Receiver<PacerArrival>,
    /// Clock completion with the number of arrivals rejected by its queue.
    clock: JoinHandle<u32>,
    /// Total independently scheduled arrivals.
    count: u32,
}

impl Pacer {
    /// Start an independent clock for a validated positive arrival rate.
    ///
    /// # Errors
    /// Returns an arrival-count overflow.
    fn new(point: Point, start: Instant) -> Result<Self> {
        let count = point
            .amount
            .checked_mul(point.seconds)
            .context("arrival count overflow")?;
        let (sender, arrivals) = mpsc::channel(PACER_PENDING_ARRIVALS);
        let clock = tokio::task::spawn_blocking(move || {
            let mut dropped = 0;
            for sequence in 0..count {
                let scheduled =
                    start + Duration::from_secs_f64(f64::from(sequence) / f64::from(point.amount));
                std::thread::sleep(scheduled.saturating_duration_since(Instant::now()));
                match sender.try_send(PacerArrival {
                    sequence,
                    scheduled,
                }) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => dropped += 1,
                    // Dropping the receiver cancels the clock at its next deadline.
                    Err(mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
            dropped
        });
        Ok(Self {
            arrivals,
            clock,
            count,
        })
    }
}

// -----------------------------------------------------------------------------
// Load: Bounded client tasks and raw measurement recording.
// -----------------------------------------------------------------------------

/// Request deadline and maximum open-model client occupancy in seconds.
pub(super) const LOAD_REQUEST_SECONDS: u32 = 10;

/// State of one independent measurement window.
pub(super) struct Load<'a> {
    /// Reused connection pool.
    client: &'a Client,
    /// Prepared deterministic request corpus.
    workloads: &'a Workloads,
    /// Load policy for this window.
    point: Point,
    /// Beginning of the offered-load window.
    start: Instant,
    /// Deadline after which only draining occurs.
    end: Instant,
    /// Accepted requests currently awaiting collection.
    tasks: JoinSet<Sample>,
    /// Number of scheduled arrivals, including drops.
    offered: u32,
    /// Scheduled arrivals that could not be dispatched on time.
    dropped: u32,
    /// Largest observed set of pending requests.
    peak_concurrency: usize,
    /// Raw sample writer and histograms.
    measurements: Measurements,
}

impl<'a> Load<'a> {
    /// Initialize raw recording before the measurement clock starts.
    ///
    /// # Errors
    /// Returns file or histogram initialization errors.
    pub fn new(
        client: &'a Client,
        workloads: &'a Workloads,
        point: Point,
        directory: &Path,
    ) -> Result<Self> {
        let measurements = Measurements::new(&directory.join("requests.jsonl"))?;
        let start = Instant::now();
        Ok(Self {
            client,
            workloads,
            point,
            start,
            end: start + Duration::from_secs(u64::from(point.seconds)),
            tasks: JoinSet::new(),
            offered: 0,
            dropped: 0,
            peak_concurrency: 0,
            measurements,
        })
    }

    /// Offer one request using a globally ordered fixture sequence.
    fn spawn(&mut self, scheduled: Instant) {
        let workload = self.workloads.selected(u64::from(self.offered));
        self.offered += 1;
        let client = self.client.clone();
        self.tasks
            .spawn(async move { workload.perform(&client, scheduled).await });
        self.peak_concurrency = self.peak_concurrency.max(self.tasks.len());
    }

    /// Replace completed requests until the fixed-concurrency window ends.
    ///
    /// # Errors
    /// Returns task failures or raw recording errors.
    async fn closed(&mut self) -> Result<()> {
        for _ in 0..self.point.amount {
            self.spawn(Instant::now());
        }
        while let Some(sample) = self.tasks.join_next().await {
            self.measurements.record(&sample?, self.end)?;
            if Instant::now() >= self.end {
                // Drain existing clients without offering more work.
                continue;
            }
            self.spawn(Instant::now());
        }
        Ok(())
    }

    /// Collect responses until an independent arrival or the clock's completion.
    ///
    /// # Errors
    /// Returns task or recording failures.
    async fn next_arrival(&mut self, pacer: &mut Pacer) -> Result<Option<PacerArrival>> {
        while !self.tasks.is_empty() {
            let arrival = Box::pin(pacer.arrivals.recv());
            let completion = Box::pin(self.tasks.join_next());
            match select(arrival, completion).await {
                // Deliver the next tick, or the end of its scheduled window.
                Either::Left((arrival, _)) => return Ok(arrival),
                Either::Right((Some(sample), _)) => self.measurements.record(&sample?, self.end)?,
                Either::Right((None, _)) => {}
            }
        }
        Ok(pacer.arrivals.recv().await)
    }

    /// Offer independent arrivals, counting missed deadlines and full occupancy.
    ///
    /// # Errors
    /// Returns task, scheduling arithmetic, or raw recording errors.
    async fn open(&mut self) -> Result<()> {
        let period = Duration::from_secs_f64(1.0 / f64::from(self.point.amount));
        let limit = usize::try_from(self.point.amount)? * usize::try_from(LOAD_REQUEST_SECONDS)?;
        let mut pacer = Pacer::new(self.point, self.start)?;
        while let Some(arrival) = self.next_arrival(&mut pacer).await? {
            self.offered = arrival.sequence;
            if self.tasks.len() >= limit || Instant::now() > arrival.scheduled + period {
                self.offered += 1;
                self.dropped += 1;

                // Do not turn missed arrivals into a catch-up burst.
                continue;
            }
            self.spawn(arrival.scheduled);
        }
        self.offered = pacer.count;
        self.dropped += pacer.clock.await.context("arrival clock failed")?;
        sleep_until(self.end).await;
        while let Some(sample) = self.tasks.join_next().await {
            self.measurements.record(&sample?, self.end)?;
        }
        Ok(())
    }

    /// Run the selected scheduling model and report complete and in-window work.
    ///
    /// # Errors
    /// Returns scheduling, task, or artifact errors.
    pub async fn run(mut self) -> Result<Value> {
        match self.point.mode {
            Mode::Closed => self.closed().await?,
            Mode::Open => self.open().await?,
        }
        let elapsed = self.start.elapsed().as_secs_f64();
        let mut summary = self.measurements.summary()?;
        summary["offered"] = json!(self.offered);
        summary["dropped"] = json!(self.dropped);
        summary["peak_concurrency"] = json!(self.peak_concurrency);
        summary["elapsed_seconds"] = json!(elapsed);
        summary["received_bytes_per_second"] =
            json!(summary["received_bytes"].as_f64().unwrap_or(0.0) / elapsed);
        summary["successful_rps"] =
            json!(f64::from(self.measurements.completed_in_window) / f64::from(self.point.seconds));
        summary["successful_rps_including_drain"] =
            json!(f64::from(self.measurements.successful) / elapsed);
        Ok(summary)
    }
}

// -----------------------------------------------------------------------------
// Tests: Verifies bounded independent arrival scheduling.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Rate at which Tokio's millisecond timer previously lost most arrivals.
    const TEST_ARRIVALS_PER_SECOND: u32 = 1000;

    /// Brief real-clock window for bounded-queue accounting.
    const TEST_WINDOW_SECONDS: u32 = 1;

    /// A stalled consumer retains one arrival and counts every other clock tick.
    ///
    /// # Errors
    /// Returns clock task or missing-arrival failures.
    ///
    /// # Panics
    /// Panics if the queue grows, loses accounting, or changes the global sequence.
    #[tokio::test]
    async fn bounded_clock_counts_unconsumed_arrivals() -> Result<()> {
        let Pacer {
            mut arrivals,
            clock,
            count,
        } = Pacer::new(
            Point {
                mode: Mode::Open,
                amount: TEST_ARRIVALS_PER_SECOND,
                seconds: TEST_WINDOW_SECONDS,
            },
            Instant::now(),
        )?;
        let dropped = clock.await?;
        let first = arrivals.recv().await.context("first arrival")?;
        assert_eq!(first.sequence, 0);
        assert!(arrivals.recv().await.is_none());
        assert_eq!(dropped + 1, count);
        Ok(())
    }
}
