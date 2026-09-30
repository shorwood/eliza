//! Runs the Hurl HTTP contracts against a compiled ELIZA server.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{io, thread};

// -----------------------------------------------------------------------------
// Startup: Bounds server readiness probes.
// -----------------------------------------------------------------------------

/// Maximum time allowed for the server to bind its test port.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

/// Delay between server readiness probes.
const STARTUP_RETRY: Duration = Duration::from_millis(20);

// -----------------------------------------------------------------------------
// TestServer: Owns one isolated HTTP test process.
// -----------------------------------------------------------------------------

/// Spawned ELIZA process bound to an ephemeral local address.
struct TestServer {
    /// Child process stopped and reaped when the test finishes.
    child: Child,
    /// Root URL passed to Hurl as a template variable.
    base_url: String,
}

impl TestServer {
    /// Start the compiled server with optional additional `serve` arguments.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the port, process, or readiness probe fails.
    fn spawn(extra_args: &[&str]) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        drop(listener);

        // Launch the server without polluting Hurl's contract output.
        let mut command = Command::new(env!("CARGO_BIN_EXE_eliza"));
        command.args(["serve", "--host", "127.0.0.1", "--port"]);
        command.arg(address.port().to_string());
        command.args(extra_args);
        command.stdout(Stdio::null());
        command.stderr(Stdio::inherit());

        // Prove the child is ready before handing its address to Hurl.
        let mut child = command.spawn()?;
        wait_until_ready(&mut child, address)?;

        Ok(Self {
            child,
            base_url: format!("http://{address}"),
        })
    }

    /// Run every Hurl file below `contracts` against this server.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when Hurl cannot run or a contract fails.
    fn run_hurl(&self, contracts: &Path, token: Option<&str>) -> io::Result<()> {
        let mut command = Command::new("hurl");
        command.args(["--test", "--jobs", "1", "--error-format", "long"]);
        command.args(["--connect-timeout", "2s", "--max-time", "5s"]);
        command.args(["--variable", &format!("base_url={}", self.base_url)]);
        if let Some(token) = token {
            command.args(["--variable", &format!("token={token}")]);
        }
        command.arg(contracts);

        let output = command.output().map_err(|error| {
            io::Error::other(format!(
                "failed to run Hurl; install it or enter `nix develop`: {error}"
            ))
        })?;

        // Preserve Hurl's detailed diagnostics when a contract fails.
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "Hurl contracts failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill()
            && error.kind() != io::ErrorKind::InvalidInput
        {
            eprintln!("failed to stop ELIZA test server: {error}");
        }
        match self.child.wait() {
            Ok(_) => {}
            Err(error) => eprintln!("failed to reap ELIZA test server: {error}"),
        }
    }
}

// -----------------------------------------------------------------------------
// WaitUntilReady: Waits for the server before running contracts.
// -----------------------------------------------------------------------------

/// Wait until the child accepts local TCP connections or exits.
///
/// # Errors
///
/// Returns an I/O error when the child exits, cannot be inspected, or times out.
fn wait_until_ready(child: &mut Child, address: SocketAddr) -> io::Result<()> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        // Accepting a connection proves the server is ready for Hurl.
        if TcpStream::connect_timeout(&address, STARTUP_RETRY).is_ok() {
            return Ok(());
        }

        // An exited child cannot become ready on a later probe.
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "ELIZA server exited during startup with {status}"
            )));
        }

        // Bound startup failures instead of leaving the test process hanging.
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "ELIZA server did not become ready",
            ));
        }
        thread::sleep(STARTUP_RETRY);
    }
}

// -----------------------------------------------------------------------------
// HttpContracts: Exercises public and authenticated route surfaces.
// -----------------------------------------------------------------------------

/// Run the unauthenticated HTTP contracts.
///
/// # Errors
///
/// Returns an I/O error when the server or Hurl fails.
#[test]
fn http_contracts_public() -> io::Result<()> {
    let server = TestServer::spawn(&[])?;
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/http/public");
    server.run_hurl(&contracts, None)
}

/// Run the authenticated HTTP contracts.
///
/// # Errors
///
/// Returns an I/O error when the server or Hurl fails.
#[test]
fn http_contracts_authenticated() -> io::Result<()> {
    let server = TestServer::spawn(&["--auth", "bearer", "--bearer-token", "secret"])?;
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/http/auth");
    server.run_hurl(&contracts, Some("secret"))
}
