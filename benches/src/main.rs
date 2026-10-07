//! Reproducible generation-server load tests, outside the production server.
#![feature(register_tool)]
#![register_tool(rlib)]
#![allow(
    rlib::revalidated_string_parameters,
    reason = "mixed corpus routing delegates preset validation rather than repeating it"
)]

mod load;
mod metrics;
mod workload;

use std::fs::{self, File};
use std::net::TcpListener;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use futures_util::future::{Either, select};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::time::{Instant, sleep};

use crate::load::{LOAD_REQUEST_SECONDS, Load, Mode, Point};
use crate::metrics::{Process, save};
use crate::workload::Workloads;

// -----------------------------------------------------------------------------
// Timing: Fixed local measurement policies.
// -----------------------------------------------------------------------------

/// Readiness retry interval in milliseconds.
const TIMING_READY_RETRY_MS: u64 = 20;

/// Process CPU/RSS sampling interval in milliseconds.
const TIMING_PROCESS_SAMPLE_MS: u64 = 200;

/// Chat completion capacity budget in milliseconds.
const TIMING_CHAT_BUDGET_MS: f64 = 100.0;

/// Speech and mixed completion capacity budget in milliseconds.
const TIMING_SPEECH_BUDGET_MS: f64 = 5000.0;

// -----------------------------------------------------------------------------
// Options: Validated local benchmark configuration.
// -----------------------------------------------------------------------------

/// Validated local benchmark configuration.
#[derive(Clone, Parser)]
#[command(about = "Measure Eliza throughput, streaming latency, and CPU/request")]
struct Options {
    /// Run brief fixture checks instead of the full baseline.
    #[arg(long = "smoke")]
    is_smoke: bool,
    /// Comma-separated request workloads to measure.
    #[arg(
        long,
        default_value = "chat-short,chat-long,chat-sse,speech-medium-pcm,mixed",
        value_delimiter = ','
    )]
    workloads: Vec<String>,
    /// Concurrent client counts for the closed-model sweep.
    #[arg(long, default_value = "1,2,4,8,16,32,64", value_delimiter = ',')]
    concurrency: Vec<u32>,
    /// Requests per second for the open-model sweep.
    #[arg(long, default_value = "10,50,100,250,500", value_delimiter = ',')]
    rates: Vec<u32>,
    /// Measurement duration in seconds per load point.
    #[arg(long, default_value = "60")]
    duration: u32,
    /// Warmup duration in seconds per load point.
    #[arg(long, default_value = "10")]
    warmup: u32,
    /// Independent repetitions of each load point.
    #[arg(long, default_value = "3")]
    repeats: u32,
    /// Duration of the separate streaming probes; zero skips them.
    #[arg(long, default_value = "60")]
    probe_seconds: u32,
    /// Logical CPUs reserved for the server.
    #[arg(long, value_delimiter = ',')]
    server_cpus: Vec<usize>,
    /// Logical CPUs reserved for the load generator.
    #[arg(long, value_delimiter = ',')]
    client_cpus: Vec<usize>,
    /// Async server worker counts to compare (default: server CPU count).
    #[arg(long, value_delimiter = ',')]
    runtime_workers: Vec<NonZeroUsize>,
    /// Concurrent speech job limits to compare.
    #[arg(long, value_delimiter = ',', default_value = "2")]
    speech_workers: Vec<NonZeroUsize>,
    /// Fresh directory for raw samples and summaries.
    #[arg(long)]
    output: Option<PathBuf>,
}

impl Options {
    /// Use brief workload checks without timing assertions.
    fn smoke(&mut self) {
        self.duration = 1;
        self.warmup = 1;
        self.repeats = 1;
        self.probe_seconds = 2;
        self.concurrency = vec![1];
        self.rates = vec![1];
        self.workloads = vec![
            "chat-short".to_owned(),
            "chat-long".to_owned(),
            "chat-sse".to_owned(),
            "chat-long-sse".to_owned(),
            "mixed".to_owned(),
        ];
        for size in ["short", "medium", "long"] {
            for format in ["pcm", "wav", "mp3"] {
                self.workloads.extend([
                    format!("speech-{size}-{format}"),
                    format!("speech-{size}-{format}-sse"),
                ]);
            }
        }
    }

    /// Validate CPU selections and measurement bounds.
    ///
    /// # Errors
    /// Returns invalid configuration or affinity failures.
    fn configure(&mut self) -> Result<()> {
        let available = available_cpus()?;
        if self.server_cpus.is_empty() {
            self.server_cpus = available[..(available.len() / 2).max(1)].to_vec();
        }
        if self.client_cpus.is_empty() {
            self.client_cpus = available[(available.len() / 2).max(1)..].to_vec();
        }
        ensure!(
            !self.client_cpus.is_empty(),
            "need at least two available CPUs or explicit shared CPU selections"
        );
        ensure!(
            self.server_cpus
                .iter()
                .chain(&self.client_cpus)
                .all(|cpu| available.contains(cpu)),
            "CPU selections exceed current affinity"
        );
        if self.is_smoke {
            self.smoke();
        }
        if self.runtime_workers.is_empty() {
            self.runtime_workers
                .push(NonZeroUsize::new(self.server_cpus.len()).context("server CPUs")?);
        }
        self.runtime_workers.sort_unstable();
        self.runtime_workers.dedup();
        self.speech_workers.sort_unstable();
        self.speech_workers.dedup();
        ensure!(
            self.speech_workers
                .iter()
                .all(|workers| workers.get() <= tokio::sync::Semaphore::MAX_PERMITS),
            "speech workers exceed semaphore capacity"
        );
        ensure!(
            self.duration > 0 && self.repeats > 0 && !self.workloads.is_empty(),
            "duration, repeats, workloads must be nonempty"
        );
        ensure!(
            self.concurrency
                .iter()
                .chain(&self.rates)
                .all(|amount| *amount > 0),
            "load points must be positive"
        );
        Ok(())
    }

    /// Enumerate workload and load-level combinations.
    #[allow(
        rlib::nested_tuple_types,
        reason = "a workload/load-level Cartesian product needs no additional domain type"
    )]
    fn points(&self) -> Vec<(String, Mode, u32)> {
        let levels = [(Mode::Closed, &self.concurrency), (Mode::Open, &self.rates)];
        let levels = levels
            .into_iter()
            .flat_map(|(mode, amounts)| amounts.iter().map(move |amount| (mode, *amount)))
            .collect::<Vec<_>>();
        let mut cases = Vec::new();
        for name in &self.workloads {
            for (mode, amount) in &levels {
                cases.push((name.clone(), *mode, *amount));
            }
        }
        cases
    }

    /// Capture machine and fixture metadata.
    ///
    /// # Errors
    /// Returns metadata artifact errors.
    fn metadata(&self, root: &Path, output: &Path) -> Result<()> {
        let read =
            |path| fs::read_to_string(path).unwrap_or_else(|error| format!("unavailable: {error}"));
        let cpus = self.server_cpus.iter().chain(&self.client_cpus);
        let topology = cpus.map(|cpu| {
            let prefix = format!("/sys/devices/system/cpu/cpu{cpu}");
            (*cpu, json!({
                "governor":read(PathBuf::from(format!("{prefix}/cpufreq/scaling_governor"))),
                "min_khz":read(PathBuf::from(format!("{prefix}/cpufreq/scaling_min_freq"))),
                "max_khz":read(PathBuf::from(format!("{prefix}/cpufreq/scaling_max_freq"))),
                "siblings":read(PathBuf::from(format!("{prefix}/topology/thread_siblings_list")))
            }))
        }).collect::<std::collections::BTreeMap<_, _>>();
        save(
            &output.join("metadata.json"),
            &json!({"commit":version(&["git","rev-parse","HEAD"]),
        "diff":version(&["git","diff"]), "rust":version(&["rustc","-vV"]),
        "lockfile":read(root.join("Cargo.lock")), "fixtures":read(root.join("benches/fixtures.json")),
        "cpu":read(PathBuf::from("/proc/cpuinfo")), "memory":read(PathBuf::from("/proc/meminfo")),
        "process_limits":read(PathBuf::from("/proc/self/limits")),
        "cpu_max":read(PathBuf::from("/sys/fs/cgroup/cpu.max")), "os":version(&["uname","-a"]),
        "cgroup":read(PathBuf::from("/proc/self/cgroup")), "topology":topology,
        "benchmark_manifest":read(root.join("benches/Cargo.toml")),
        "server_cpus":self.server_cpus, "client_cpus":self.client_cpus,
        "runtime_workers":self.runtime_workers, "speech_workers":self.speech_workers,
        "workspace_manifest":read(root.join("Cargo.toml")),
        "duration":self.duration, "warmup":self.warmup, "repeats":self.repeats,
        "probe_seconds":self.probe_seconds,
        "loopback":true, "cpu_sets_overlap":self.server_cpus.iter().any(|cpu| self.client_cpus.contains(cpu))}),
        )?;
        Ok(())
    }

    /// Start an independent server, warm it, measure it, and clean up.
    ///
    /// # Errors
    /// Returns setup, validation, or I/O failures to the benchmark runner.
    fn run_point(
        &self,
        root: &Path,
        directory: &Path,
        probe_output: Option<&Path>,
        name: &str,
        point: Point,
    ) -> Result<Value> {
        let Point { mode, amount, .. } = point;
        let mut server = Server::new(self, root, directory)?;
        let workloads = Workloads::new(name, &server.base)?;
        affinity(&self.client_cpus)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(self.client_cpus.len())
            .enable_all()
            .build()?;
        let future = async {
            let client = Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(u64::from(LOAD_REQUEST_SECONDS)))
                .build()?;
            server.ready(&client).await?;
            if let Some(output) = probe_output {
                smoke_providers(&client, &server.base).await?;
                probes(&client, &server.base, output, self.probe_seconds).await?;
            }
            if self.warmup > 0 {
                let warmup = directory.join("warmup");
                fs::create_dir(&warmup)?;
                Load::new(
                    &client,
                    &workloads,
                    Point {
                        mode,
                        amount,
                        seconds: self.warmup,
                    },
                    &warmup,
                )?
                .run()
                .await?;
            }
            server.measure(&client, &workloads, point, directory).await
        };
        let result = runtime.block_on(interruptible(future));
        if let Err(error) = &result {
            save(
                &directory.join("failure.json"),
                &json!({"error":error.to_string()}),
            )?;
        }
        result
    }

    /// Measure one repetition for one worker configuration.
    ///
    /// # Errors
    /// Returns server, request, or artifact failures.
    fn run(&self, root: &Path, output: &Path, repetition: u32) -> Result<()> {
        let results_path = output.join("results.json");
        let mut results: Vec<Value> = if results_path.exists() {
            serde_json::from_reader(File::open(&results_path)?)?
        } else {
            Vec::new()
        };
        let mut combinations = self.points();
        ensure!(!combinations.is_empty(), "no load points selected");
        if repetition % 2 == 1 {
            combinations.reverse();
        }
        for (name, mode, amount) in combinations {
            let directory = output.join(format!("{name}-{mode:?}-{amount}-r{}", repetition + 1));
            fs::create_dir(&directory)?;
            let point = Point {
                mode,
                amount,
                seconds: self.duration,
            };
            let mut result = self.run_point(
                root,
                &directory,
                results.is_empty().then_some(output),
                &name,
                point,
            )?;
            result["workload"] = json!(name);
            result["mode"] = json!(format!("{mode:?}"));
            result["amount"] = json!(amount);
            result["repetition"] = json!(repetition + 1);
            result["runtime_workers"] = json!(self.runtime_workers.first());
            result["speech_workers"] = json!(self.speech_workers.first());
            result["within_budget"] = json!(is_within_budget(&name, &result));
            ensure!(
                !self.is_smoke || (result["failures"] == 0 && result["dropped"] == 0),
                "benchmark smoke failed: {name}"
            );
            println!(
                "{name} {mode:?} {amount} r{}: {} successful RPS, p95 {} ms, failures {}, drops {}",
                repetition + 1,
                result["successful_rps"],
                result["completion_ms"]["p95"],
                result["failures"],
                result["dropped"]
            );
            save(&directory.join("summary.json"), &result)?;
            results.push(result);
            save(&output.join("results.json"), &results)?;
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Server: Owned server process stopped and reaped after each load point.
// -----------------------------------------------------------------------------

/// Owned server process stopped and reaped after each load point.
struct Server {
    /// Child process owned by this measurement.
    child: Child,
    /// Provider route base for this server.
    base: String,
}

impl Server {
    /// Launch a fresh server with inherited CPU affinity and fixed worker count.
    ///
    /// # Errors
    /// Returns listener allocation, affinity, or process launch failures.
    fn new(options: &Options, root: &Path, directory: &Path) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        drop(listener);
        let base = format!("http://{address}");
        affinity(&options.server_cpus)?;
        let server_log = File::create(directory.join("server.log"))?;
        let mut command = Command::new(root.join("target/release/eliza"));
        command.args([
            "serve",
            "--bind",
            &address.to_string(),
            "--stream-delay-ms",
            "0",
        ]);
        for workers in &options.runtime_workers {
            command.args(["--runtime-workers", &workers.to_string()]);
        }
        for workers in &options.speech_workers {
            command.args(["--speech-workers", &workers.to_string()]);
        }
        command
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(server_log);
        let child = command.spawn()?;
        Ok(Self { child, base })
    }

    /// Wait for an HTTP health response before offering load.
    ///
    /// # Errors
    /// Returns startup failure or readiness timeout.
    async fn ready(&mut self, client: &Client) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(u64::from(LOAD_REQUEST_SECONDS));
        let mut is_ready = false;
        while !is_ready && Instant::now() < deadline {
            ensure!(
                self.child.try_wait()?.is_none(),
                "server exited before readiness"
            );
            let response = client.get(format!("{}/healthz", self.base)).send().await;

            is_ready = response.is_ok_and(|response| response.status() == 200);
            sleep(Duration::from_millis(TIMING_READY_RETRY_MS)).await;
        }
        ensure!(is_ready, "server readiness timed out");
        Ok(())
    }

    /// Read counters only while the owned server is alive.
    ///
    /// # Errors
    /// Returns the server exit status or a process sampling error.
    fn snapshot(&mut self) -> Result<Process> {
        let status = self.child.try_wait()?;
        ensure!(
            status.is_none(),
            "server exited: {status:?}; inspect server.log"
        );
        Process::read(self.child.id())
    }

    /// Collect process CPU and memory alongside one load point.
    ///
    /// # Errors
    /// Returns sampling, request, or artifact failures.
    async fn measure(
        &mut self,
        client: &Client,
        workloads: &Workloads,
        point: Point,
        directory: &Path,
    ) -> Result<Value> {
        let before = self.snapshot()?;
        let client_before = Process::read(std::process::id())?;
        let start = Instant::now();
        let mut interval = tokio::time::interval(Duration::from_millis(TIMING_PROCESS_SAMPLE_MS));
        let mut snapshots = Vec::new();
        let mut peak_rss = before.rss_bytes;
        let future = Load::new(client, workloads, point, directory)?.run();
        tokio::pin!(future);
        let mut result = loop {
            match futures_util::future::select(&mut future, Box::pin(interval.tick())).await {
                futures_util::future::Either::Left((result, _)) => break result?,
                futures_util::future::Either::Right(_) => {
                    let server = self.snapshot()?;
                    peak_rss = peak_rss.max(server.rss_bytes);
                    snapshots.push(json!({"elapsed_seconds":start.elapsed().as_secs_f64(),
                    "server":server, "client":Process::read(std::process::id())?}));
                }
            }
        };
        let after = self.snapshot()?;
        let client_after = Process::read(std::process::id())?;
        ensure!(
            self.child.try_wait()?.is_none(),
            "server exited during measurement"
        );
        let user = after.user_seconds - before.user_seconds;
        let system = after.system_seconds - before.system_seconds;
        result["user_seconds"] = json!(user);
        result["system_seconds"] = json!(system);
        result["client_cpu_seconds"] = json!(
            client_after.user_seconds + client_after.system_seconds
                - client_before.user_seconds
                - client_before.system_seconds
        );
        result["peak_sampled_rss_bytes"] = json!(peak_rss.max(after.rss_bytes));
        result["cpu_seconds_per_success"] = result["successful"]
            .as_u64()
            .filter(|count| *count > 0)
            .map_or(Value::Null, |count| {
                json!((user + system) / f64::from(u32::try_from(count).unwrap_or(u32::MAX)))
            });
        save(&directory.join("process.json"), &snapshots)?;
        Ok(result)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill() {
            eprintln!("server cleanup: {error}");
        }
        match self.child.wait() {
            Ok(_) => {}
            Err(error) => eprintln!("server reap: {error}"),
        }
    }
}

// -----------------------------------------------------------------------------
// SmokeProviders: Verify the other provider routes before measuring OpenAI.
// -----------------------------------------------------------------------------

/// Verify the other provider routes before measuring `OpenAI`.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
async fn smoke_providers(client: &Client, base: &str) -> Result<()> {
    for (path, body, pointer) in [
        (
            "/anthropic/v1/messages",
            json!({"model":"eliza-1966", "max_tokens":100,
            "messages":[{"role":"user","content":"I need help"}]}),
            "/content/0/text",
        ),
        (
            "/gemini/v1beta/models/eliza-1966:generateContent",
            json!({"contents":[{"role":"user", "parts":[{"text":"I need help"}]}]}),
            "/candidates/0/content/parts/0/text",
        ),
        (
            "/ollama/api/chat",
            json!({"model":"eliza-1966", "stream":false,
            "messages":[{"role":"user","content":"I need help"}]}),
            "/message/content",
        ),
    ] {
        let response: Value = client
            .post(format!("{base}{path}"))
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        ensure!(
            response
                .pointer(pointer)
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty()),
            "invalid provider response: {path}"
        );
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Probes: Measure payload and completion latency at one request per second.
// -----------------------------------------------------------------------------

/// Measure payload and completion latency at one request per second.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
async fn probes(client: &Client, base: &str, directory: &Path, seconds: u32) -> Result<()> {
    for name in ["chat-sse", "speech-medium-pcm", "speech-medium-pcm-sse"]
        .into_iter()
        .filter(|_| seconds > 0)
    {
        let workloads = Workloads::new(name, base)?;
        let path = directory.join(format!("probe-{name}"));
        fs::create_dir(&path)?;
        let summary = Load::new(
            client,
            &workloads,
            Point {
                mode: Mode::Open,
                amount: 1,
                seconds,
            },
            &path,
        )?
        .run()
        .await?;
        ensure!(
            summary["failures"] == 0 && summary["dropped"] == 0,
            "streaming probe failed: {name}"
        );
        save(&path.join("summary.json"), &summary)?;
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Version: Record tool and host versions for reproducibility.
// -----------------------------------------------------------------------------

/// Record tool and host versions for reproducibility.
fn version(args: &[&str]) -> String {
    Command::new(args[0]).args(&args[1..]).output().map_or_else(
        |error| error.to_string(),
        |output| String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    )
}

// -----------------------------------------------------------------------------
// Affinity: Apply a CPU mask inherited by new workers and children.
// -----------------------------------------------------------------------------

/// Apply a CPU mask inherited by new workers and children.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
#[cfg(target_os = "linux")]
fn affinity(cpus: &[usize]) -> Result<()> {
    let mut mask = nix::sched::CpuSet::new();
    for cpu in cpus {
        mask.set(*cpu)?;
    }
    nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &mask)?;
    Ok(())
}

/// Apply a CPU mask inherited by new workers and children.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
#[cfg(not(target_os = "linux"))]
fn affinity(_: &[usize]) -> Result<()> {
    anyhow::bail!("automatic affinity and CPU collection require Linux")
}

// -----------------------------------------------------------------------------
// AvailableCpus: Read the current process CPU allowance.
// -----------------------------------------------------------------------------

/// Read the current process CPU allowance.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
#[cfg(target_os = "linux")]
fn available_cpus() -> Result<Vec<usize>> {
    let mask = nix::sched::sched_getaffinity(nix::unistd::Pid::from_raw(0))?;
    Ok((0..nix::sched::CpuSet::count())
        .filter(|cpu| mask.is_set(*cpu).unwrap_or(false))
        .collect())
}

/// Read the current process CPU allowance.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
#[cfg(not(target_os = "linux"))]
fn available_cpus() -> Result<Vec<usize>> {
    anyhow::bail!("automatic load runner requires Linux")
}

// -----------------------------------------------------------------------------
// Interruptible: Clean up the owned server when a run is interrupted.
// -----------------------------------------------------------------------------

/// Cancel an HTTP run on Ctrl-C so the server ownership guard can reap it.
///
/// # Errors
/// Returns measurement errors or an explicit interruption error.
async fn interruptible(future: impl std::future::Future<Output = Result<Value>>) -> Result<Value> {
    let measurement = Box::pin(future);
    let interrupt = Box::pin(tokio::signal::ctrl_c());
    match select(measurement, interrupt).await {
        Either::Left((result, _)) => result,
        Either::Right((signal, _)) => {
            signal?;
            anyhow::bail!("benchmark interrupted; completed artifacts were retained")
        }
    }
}

// -----------------------------------------------------------------------------
// Main: Run the requested sweeps and retain results after each point.
// -----------------------------------------------------------------------------

/// Run the requested sweeps and retain results after each point.
///
/// # Errors
/// Returns setup, validation, or I/O failures to the benchmark runner.
fn main() -> Result<()> {
    let mut options = Options::parse();
    options.configure()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("workspace root")?;
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let output = options
        .output
        .clone()
        .unwrap_or_else(|| root.join(format!("target/benchmarks/{stamp}")));
    ensure!(!output.exists(), "output directory already exists");
    fs::create_dir_all(&output)?;
    options.metadata(root, &output)?;
    let is_matrix = options.runtime_workers.len() * options.speech_workers.len() > 1;
    let workers = options
        .runtime_workers
        .iter()
        .flat_map(|runtime| {
            options
                .speech_workers
                .iter()
                .map(move |speech| (*runtime, *speech))
        })
        .collect::<Vec<_>>();
    let mut configurations = Vec::new();
    for (runtime_workers, speech_workers) in workers {
        let mut configuration = options.clone();
        configuration.runtime_workers = vec![runtime_workers];
        configuration.speech_workers = vec![speech_workers];
        let directory = if is_matrix {
            output.join(format!("runtime-{runtime_workers}-speech-{speech_workers}"))
        } else {
            output.clone()
        };
        if is_matrix {
            fs::create_dir(&directory)?;
            configuration.metadata(root, &directory)?;
        }
        configurations.push((configuration, directory));
    }
    for repetition in 0..options.repeats {
        if repetition > 0 {
            configurations.reverse();
        }
        for (configuration, directory) in &configurations {
            configuration.run(root, directory, repetition)?;
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// WithinBudget: Declared capacity acceptance for one workload.
// -----------------------------------------------------------------------------

/// Check the declared latency budget, including failed and dropped work.
fn is_within_budget(name: &str, result: &Value) -> bool {
    let budget = if name.starts_with("speech") || name == "mixed" {
        TIMING_SPEECH_BUDGET_MS
    } else {
        TIMING_CHAT_BUDGET_MS
    };
    result["failures"] == 0
        && result["dropped"] == 0
        && result["completion_ms"]["p95"]
            .as_f64()
            .is_some_and(|latency| latency <= budget)
}
