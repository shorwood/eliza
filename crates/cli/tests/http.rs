//! Runs the embedded Hurl contracts against isolated ELIZA configurations.

#[path = "support/hosted.rs"]
mod hosted;

use std::net::{SocketAddr, TcpListener, TcpStream};
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
// HurlVariable: Pass generated and shared values to fixtures.
// -----------------------------------------------------------------------------

/// Value injected into an HTTP fixture.
struct HurlVariable<'value> {
    /// Placeholder name without braces.
    name: &'value str,
    /// String substituted into the fixture.
    value: &'value str,
}

impl<'value> HurlVariable<'value> {
    /// Name one value passed to a Hurl fixture.
    const fn new(name: &'value str, value: &'value str) -> Self {
        Self { name, value }
    }
}

/// Authentication value shared by the secured fixture groups.
const HURL_VARIABLE_TOKEN: &[HurlVariable<'static>] = &[HurlVariable::new("token", "secret")];

// -----------------------------------------------------------------------------
// Alias: Exercise renamed, repeated, and deduplicated model IDs.
// -----------------------------------------------------------------------------

/// CLI options for the complete alias contract.
const ALIAS_ARGS: &[&str] = &[
    "--api-key",
    "secret",
    "--chat-model",
    "chat-one",
    "--model-chat",
    "chat-two",
    "--model-chat",
    "chat-one",
    "--model-embeddings",
    "embed-one",
    "--model-embeddings",
    "embed-two",
    "--model-embeddings",
    "embed{three}",
    "--model-embeddings",
    "embed-one",
    "--model-speech",
    "voice-one",
    "--model-speech",
    "voice-two",
    "--model-speech",
    "voice-one",
    "--model-images",
    "image-one",
    "--model-images",
    "image-two",
];

/// CLI options for slash-containing model aliases.
const ALIAS_SLASH_ARGS: &[&str] = &[
    "--model-chat",
    "acme/chat",
    "--model-embeddings",
    "acme/embed",
];

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
        Self::spawn_with_env(extra_args, &std::collections::HashMap::new())
    }

    /// Start one isolated server with process-local fixture secrets.
    /// # Errors
    /// Returns process or listener startup failures.
    fn spawn_with_env(
        extra_args: &[&str],
        environment: &std::collections::HashMap<&str, &str>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let address = listener.local_addr()?;
        drop(listener);

        // Launch the server without polluting Hurl's contract output.
        let mut command = Command::new(env!("CARGO_BIN_EXE_eliza"));
        command.args(["serve", "--bind"]);
        command.arg(address.to_string());
        command.args(extra_args);
        command.envs(environment);
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

    /// Run one fixture selection against this server.
    ///
    /// # Errors
    /// Returns an error when fixture discovery or a contract fails.
    fn run_hurl(&self, pattern: &str, variables: &[HurlVariable<'_>]) -> io::Result<()> {
        let cases = hurl_test::TestCases::new();
        cases.variable("base_url", &self.base_url);
        for variable in variables {
            cases.variable(variable.name, variable.value);
        }
        cases.include(pattern).map_err(io::Error::other)?;
        cases.run().map_err(io::Error::other)
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
// HttpTest: Runs every fixture against its required server profile.
// -----------------------------------------------------------------------------

/// Run every HTTP contract through one integration test.
///
/// # Errors
/// Returns an I/O error when a server or contract fails.
#[test]
fn http_test() -> io::Result<()> {
    // Generate large payloads here; Hurl owns their requests and assertions.
    let encoded_image = "A".repeat(2 * 1024 * 1024 + 4);
    let oversized_prompt = "x".repeat(8_001);
    let public_variables = [
        HurlVariable::new("oversized_image", &encoded_image),
        HurlVariable::new("oversized_prompt", &oversized_prompt),
    ];

    // Default configuration covers the public API and large generated bodies.
    let server = TestServer::spawn(&[])?;
    server.run_hurl("tests/http/public/*.hurl", &public_variables)?;
    drop(server);

    // Repeat public contracts with explicit worker counts.
    let server = TestServer::spawn(&["--runtime-workers", "1", "--speech-workers", "4"])?;
    server.run_hurl("tests/http/public/*.hurl", &public_variables)?;
    drop(server);

    // Authentication changes the expected result of unauthenticated requests.
    let server = TestServer::spawn(&["--api-key", "secret"])?;
    server.run_hurl("tests/http/auth/*.hurl", HURL_VARIABLE_TOKEN)?;
    drop(server);

    // Alias replacement requires catalogs different from the public defaults.
    let server = TestServer::spawn(ALIAS_ARGS)?;
    server.run_hurl("tests/http/aliases.hurl", HURL_VARIABLE_TOKEN)?;
    drop(server);

    // Slash aliases have their own exact catalog expectations.
    let server = TestServer::spawn(ALIAS_SLASH_ARGS)?;
    server.run_hurl("tests/http/aliases_slash.hurl", &[])?;
    drop(server);

    // Shared names require an intentional chat/embedding collision.
    let server = TestServer::spawn(&["--chat-model", "fnv-embed"])?;
    server.run_hurl("tests/http/aliases_shared.hurl", &[])?;
    drop(server);

    // Hosted mode is mutually exclusive with --api-key and uses ingress policy.
    hosted::run()
}
