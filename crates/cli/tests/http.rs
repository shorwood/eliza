//! Runs the Hurl HTTP contracts against a compiled ELIZA server.

use std::io::Write as _;
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
// JsonRequest: Distinguishes one direct test path from its body.
// -----------------------------------------------------------------------------

/// Raw JSON request submitted outside the Hurl contract runner.
#[derive(Clone, Copy)]
struct JsonRequest<'request> {
    /// Route path below the test server's root URL.
    path: &'request str,
    /// Complete JSON request body.
    body: &'request str,
}

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
        command.args(["serve", "--bind"]);
        command.arg(address.to_string());
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

    /// Submit one raw JSON request and collect the closed HTTP response.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the socket cannot send or receive the request.
    fn post_json(&self, request: JsonRequest<'_>) -> io::Result<String> {
        // Configure curl to expose headers and consume an exact stdin body.
        let url = format!("{}{}", self.base_url, request.path);
        let mut command = Command::new("curl");
        command.args(["--silent", "--include", "--request", "POST"]);
        command.args(["--header", "Content-Type: application/json"]);
        command.args(["--data-binary", "@-", &url]);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());

        // Submit the body and wait for curl to close the HTTP exchange.
        let mut child = command.spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("curl stdin was not piped"))?
            .write_all(request.body.as_bytes())?;
        let output = child.wait_with_output()?;

        // A failed curl process cannot yield a trustworthy HTTP response.
        if !output.status.success() {
            return Err(io::Error::other("curl failed to submit image body"));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
    let server = TestServer::spawn(&["--api-key", "secret"])?;
    let contracts = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/http/auth");
    server.run_hurl(&contracts, Some("secret"))
}

/// Prove image routes raise Axum's former two-mebibyte JSON body limit.
///
/// # Errors
///
/// Returns an I/O error when the server cannot process the direct request.
///
/// # Panics
///
/// Panics when the route does not accept the body or loses its typed failure.
#[test]
fn http_contracts_image_json_body_exceeds_default_limit() -> io::Result<()> {
    // Build malformed image input beyond Axum's former two-mebibyte limit.
    let server = TestServer::spawn(&[])?;
    let encoded = "A".repeat(2 * 1024 * 1024 + 4);
    let body = format!(
        "{{\"model\":\"eliza-1966\",\"messages\":[{{\"role\":\"user\",\"content\":[{{\"type\":\"image_url\",\"image_url\":{{\"url\":\"data:image/png;base64,{encoded}\"}}}}]}}]}}"
    );

    // The route must reach image validation instead of rejecting body size.
    let response = server.post_json(JsonRequest {
        path: "/openai/v1/chat/completions",
        body: &body,
    })?;

    assert!(
        response.contains("HTTP/1.1 400 Bad Request"),
        "unexpected response: {response}"
    );
    assert!(
        response.contains("x-eliza-error-code: eliza::image::malformed"),
        "unexpected response: {response}"
    );
    Ok(())
}

/// Prove image-generation prompt limits retain provider-native 413 envelopes.
///
/// # Errors
///
/// Returns an I/O error when the server cannot process either direct request.
///
/// # Panics
///
/// Panics when a provider loses the shared limit diagnostic.
#[test]
fn http_contracts_image_generation_prompt_limit() -> io::Result<()> {
    let server = TestServer::spawn(&[])?;
    let prompt = "x".repeat(8_001);
    let openai_body = format!("{{\"model\":\"eliza-retro-image\",\"prompt\":\"{prompt}\"}}");
    let openai = server.post_json(JsonRequest {
        path: "/openai/v1/images/generations",
        body: &openai_body,
    })?;
    assert!(
        openai.contains("HTTP/1.1 413 Payload Too Large"),
        "unexpected response: {openai}"
    );
    assert!(
        openai.contains("x-eliza-error-code: eliza::image::generation::prompt_too_large"),
        "unexpected response: {openai}"
    );

    let gemini_body = format!(
        "{{\"contents\":[{{\"role\":\"user\",\"parts\":[{{\"text\":\"{prompt}\"}}]}}],\"generationConfig\":{{\"responseModalities\":[\"IMAGE\"]}}}}"
    );
    let gemini = server.post_json(JsonRequest {
        path: "/gemini/v1beta/models/eliza-retro-image:generateContent",
        body: &gemini_body,
    })?;
    assert!(
        gemini.contains("HTTP/1.1 413 Payload Too Large"),
        "unexpected response: {gemini}"
    );
    assert!(
        gemini.contains("x-eliza-error-code: eliza::image::generation::prompt_too_large"),
        "unexpected response: {gemini}"
    );
    Ok(())
}
