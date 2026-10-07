//! Hosted Hurl profiles use a local restricted entitlement fixture.

use std::io::{BufRead as _, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eliza_http::hosted::HostedConfig;
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::TestServer;

// -----------------------------------------------------------------------------
// EntitlementFixture: Serves bounded local billing reads.
// -----------------------------------------------------------------------------

/// Interval between fixture accept checks.
const HOSTED_ACCEPT_RETRY_MILLIS: u64 = 10;

/// Maximum fixture input body bytes.
const HOSTED_BODY_BYTES: usize = 8192;

/// Test-only secret delivered to the child process and Hurl headers.
const HOSTED_INGRESS_SECRET: &str = "test-ingress-secret-at-least-32-bytes";

/// Separate internal read credential; never a production value.
const HOSTED_ENTITLEMENT_SECRET: &str = "test-entitlement-secret-at-least-32-bytes";

/// Two independently rotated keys belonging to the same account.
const HOSTED_KEY_A: &str =
    "eliza_test_00000000-0000-0000-0000-000000000001_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// Second key, used to prove account-level allowances.
const HOSTED_KEY_B: &str =
    "eliza_test_00000000-0000-0000-0000-000000000002_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

/// Local fake of the existing Nitro read contract, with no Stripe/network dependency.
struct EntitlementFixture {
    /// Fixture URL placed in the origin config.
    url: String,
    /// Stops the fixture accept loop.
    stopped: Arc<AtomicBool>,
    /// Owned worker, reaped before the test exits.
    worker: Option<thread::JoinHandle<()>>,
}

impl EntitlementFixture {
    /// Serve bounded test responses on an ephemeral loopback port.
    /// # Errors
    /// Returns fixture listener creation failures.
    fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopped);
        let worker = thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() {
                    // Closed clients are expected during HTTP cancellation tests.
                    match Self::respond(stream) {
                        Ok(()) | Err(_) => {}
                    }
                } else {
                    thread::sleep(Duration::from_millis(HOSTED_ACCEPT_RETRY_MILLIS));
                }
            }
        });
        Ok(Self {
            url: format!("http://{address}/billing/entitlement"),
            stopped,
            worker: Some(worker),
        })
    }

    /// Read one bounded HTTP request and send a fixture entitlement.
    /// # Errors
    /// Returns local socket or fixture encoding failures.
    fn respond(mut stream: TcpStream) -> std::io::Result<()> {
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        let mut reader = std::io::BufReader::new(stream.try_clone()?);
        let mut size = 0;
        let mut authorized = false;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line)?;
            if line == "\r\n" || line.is_empty() {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                size = value.trim().parse().unwrap_or(0);
            }
            authorized |=
                line.trim() == format!("authorization: Bearer {HOSTED_ENTITLEMENT_SECRET}");
        }

        // Never allocate from unbounded fixture input.
        if size > HOSTED_BODY_BYTES {
            return Err(std::io::Error::other("fixture body exceeded bound"));
        }
        let mut body = vec![0; size];
        reader.read_exact(&mut body)?;
        let digest = String::from_utf8_lossy(&body);
        let paid = [HOSTED_KEY_A, HOSTED_KEY_B]
            .iter()
            .any(|key| format!("{:x}", Sha256::digest(key.as_bytes())) == digest);

        // Give confirmed fixture keys a finite eligibility deadline.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_millis();

        // Match the hash-only internal billing response contract.
        let bytes = serde_json::to_vec(
            &json!({ "eligible": paid, "accountId": if paid { Some("fixture-team") } else { None },
            "eligibleUntil": now + 3_600_000, "refreshSeconds":60, "staleSeconds":300 }),
        )?;

        // Emit authentication status separately from the bounded response body.
        let status = if authorized {
            "200 OK"
        } else {
            "401 Unauthorized"
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )?;
        stream.write_all(&bytes)
    }
}

impl Drop for EntitlementFixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);

        // The worker is reaped exactly once.
        let Some(worker) = self.worker.take() else {
            return;
        };
        worker.join().expect("fixture worker must not panic");
    }
}

// -----------------------------------------------------------------------------
// HostedContracts: Verify native hosted wire behavior.
// -----------------------------------------------------------------------------

/// Run all hosted provider fixtures under one deterministic allowance profile.
/// # Errors
/// Returns fixture, child process or Hurl contract failures.
#[test]
fn hosted_contracts() -> std::io::Result<()> {
    let billing = EntitlementFixture::start()?;
    let mut config: HostedConfig =
        serde_json::from_str(include_str!("../../../../config/hosted-staging.json"))?;
    config.entitlement_url.clone_from(&billing.url);
    config.public.text.burst = 1;
    config.public.text.per_minute = 1;
    config.supporter.text.burst = 2;
    config.supporter.text.per_minute = 1;
    let mut file = tempfile::NamedTempFile::new()?;
    serde_json::to_writer(&mut file, &config)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| std::io::Error::other("fixture config path"))?;
    let server = TestServer::spawn_with_env(
        &["--hosted-config", path],
        &std::collections::HashMap::from([
            ("ELIZA_INGRESS_SECRET", HOSTED_INGRESS_SECRET),
            ("ELIZA_ENTITLEMENT_SECRET", HOSTED_ENTITLEMENT_SECRET),
        ]),
    )?;
    server.run_hurl(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/http/hosted")
            .as_path(),
        None,
    )
}
