//! End-to-end HTTP contracts for the compiled server.

#![allow(
    clippy::arbitrary_source_item_ordering,
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "HTTP test helpers stay before provider cases and failures identify broken contracts"
)]

use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use ureq::Agent;
use ureq::http::StatusCode;

/// Maximum time allowed for server startup and one HTTP request.
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Delay between server readiness probes.
const STARTUP_RETRY: Duration = Duration::from_millis(20);

/// One additional HTTP header sent by a compatibility test.
struct TestHeader<'a> {
    /// Case-insensitive HTTP field name.
    name: &'a str,
    /// Textual HTTP field value.
    value: &'a str,
}

/// Collected status and UTF-8 body from one server response.
struct TestResponse {
    /// HTTP status returned by the server.
    status: StatusCode,
    /// Fully collected response body.
    body: String,
}

impl From<ureq::http::Response<ureq::Body>> for TestResponse {
    fn from(mut response: ureq::http::Response<ureq::Body>) -> Self {
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_string()
            .expect("response body should be UTF-8");
        Self { status, body }
    }
}

/// Spawned ELIZA process and client bound to its ephemeral address.
struct TestServer {
    /// Child process stopped and reaped when the test finishes.
    child: Child,
    /// Blocking client configured with bounded requests.
    agent: Agent,
    /// Root URL for the spawned server.
    base_url: String,
}

impl TestServer {
    /// Spawn the compiled server with optional additional `serve` arguments.
    fn spawn(extra_args: &[&str]) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test port should bind");
        let port = listener
            .local_addr()
            .expect("test address should resolve")
            .port();
        drop(listener);

        // Spawn the compiled server with the ephemeral port and any extra arguments.
        let mut command = Command::new(env!("CARGO_BIN_EXE_eliza"));
        command.args(["serve", "--host", "127.0.0.1", "--port"]);
        command.arg(port.to_string());
        command.args(extra_args);
        command.stdout(Stdio::null());
        command.stderr(Stdio::piped());
        let mut child = command.spawn().expect("ELIZA server should spawn");

        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(TEST_TIMEOUT))
            .build()
            .new_agent();

        // The server may take a short time to start, so probe its health
        // endpoint until it responds or the timeout expires.
        let base_url = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + TEST_TIMEOUT;
        loop {
            // A successful health response proves the listener and router are ready.
            if agent.get(format!("{base_url}/healthz")).call().is_ok() {
                return Self {
                    child,
                    agent,
                    base_url,
                };
            }

            // If the child exited, capture its stderr and panic with diagnostics.
            if let Some(status) = child.try_wait().expect("server status should be readable") {
                let stderr = child_stderr(&mut child);
                panic!("ELIZA server exited during startup with {status}: {stderr}");
            }

            // Retry until the timeout expires, then kill and reap the child process.
            if Instant::now() >= deadline {
                child.kill().expect("timed-out server should stop");
                child.wait().expect("timed-out server should be reaped");
                let stderr = child_stderr(&mut child);
                panic!("ELIZA server did not become ready: {stderr}");
            }

            // Wait a short time before probing the server again.
            thread::sleep(STARTUP_RETRY);
        }
    }

    /// Send one GET request to the spawned server.
    fn get(&self, path: &str) -> TestResponse {
        let response = self
            .agent
            .get(format!("{}{path}", self.base_url))
            .call()
            .expect("GET request should complete");
        response.into()
    }

    /// Send one JSON POST request with optional provider headers.
    fn post_json_with(&self, path: &str, body: &Value, headers: &[TestHeader<'_>]) -> TestResponse {
        let mut request = self.agent.post(format!("{}{path}", self.base_url));
        for header in headers {
            request = request.header(header.name, header.value);
        }
        let response = request
            .send_json(body)
            .expect("POST request should complete");
        response.into()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Err(error) = self.child.kill()
            && error.kind() != std::io::ErrorKind::InvalidInput
        {
            eprintln!("failed to stop ELIZA test server: {error}");
        }
        match self.child.wait() {
            Ok(_) => {}
            Err(error) => eprintln!("failed to reap ELIZA test server: {error}"),
        }
    }
}

mod anthropic {
    use super::*;

    #[test]
    fn it_should_anthropic_message_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/messages",
            &json!({
                "model": "eliza-doctor",
                "max_tokens": 128,
                "messages": [{ "role": "user", "content": "I am sad" }]
            }),
            &[TestHeader {
                name: "anthropic-version",
                value: "2023-06-01",
            }],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["type"], "message");
        assert_eq!(body["content"][0]["type"], "text");
        assert_eq!(body["content"][0]["text"], "I AM SORRY TO HEAR YOU ARE SAD");
        assert_eq!(body["stop_reason"], "end_turn");
    }

    #[test]
    fn it_should_anthropic_stream_uses_named_sse_events() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/messages",
            &json!({
                "model": "eliza-doctor",
                "max_tokens": 128,
                "stream": true,
                "messages": [{ "role": "user", "content": "Hello" }]
            }),
            &[TestHeader {
                name: "anthropic-version",
                value: "2023-06-01",
            }],
        );
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("event: message_start"));
        assert!(body.contains("event: content_block_delta"));
        assert!(body.contains("event: message_stop"));
    }
}

mod gemini {
    use super::*;

    #[test]
    fn it_should_gemini_generate_content_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1beta/models/eliza-doctor:generateContent",
            &json!({
                "contents": [{
                    "role": "user",
                    "parts": [{ "text": "I am sad" }]
                }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["candidates"][0]["content"]["role"], "model");
        assert_eq!(
            body["candidates"][0]["content"]["parts"][0]["text"],
            "I AM SORRY TO HEAR YOU ARE SAD"
        );
        assert_eq!(body["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn it_should_gemini_stream_generate_content_can_return_sse() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse",
            &json!({
                "contents": [{
                    "role": "user",
                    "parts": [{ "text": "Hello" }]
                }]
            }),
            &[],
        );
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("data:"));
        assert!(body.contains("finishReason"));
    }

    #[test]
    fn it_should_gemini_rejects_multimodal_parts_with_gemini_error() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1beta/models/eliza-doctor:generateContent",
            &json!({
                "contents": [{
                    "role": "user",
                    "parts": [{ "inlineData": { "mimeType": "image/png", "data": "AA==" } }]
                }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
    }
}

mod openai {
    use super::*;

    #[test]
    fn it_should_openai_chat_completion_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/chat/completions",
            &json!({
                "model": "eliza-doctor",
                "messages": [{ "role": "user", "content": "I am sad" }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["object"], "chat.completion");
        assert_eq!(
            body["choices"][0]["message"]["content"],
            "I AM SORRY TO HEAR YOU ARE SAD"
        );
    }

    #[test]
    fn it_should_openai_streaming_chat_completion_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/chat/completions",
            &json!({
                "model": "eliza-doctor",
                "stream": true,
                "messages": [{ "role": "user", "content": "Hello" }]
            }),
            &[],
        );
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("chat.completion.chunk"));
        assert!(body.contains("data: [DONE]"));
    }

    #[test]
    fn it_should_gemini_openai_alias_uses_openai_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1beta/openai/chat/completions",
            &json!({
                "model": "eliza-doctor",
                "messages": [{ "role": "user", "content": "Hello" }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["object"], "chat.completion");
    }

    #[test]
    fn it_should_openai_responses_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/responses",
            &json!({
                "model": "eliza-doctor",
                "input": "I need help"
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["object"], "response");
        assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
    }

    #[test]
    fn it_should_accept_openai_responses_sdk_input() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/responses",
            &json!({
                "model": "eliza-doctor",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "I am sad" }]
                }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body["output"][0]["content"][0]["text"],
            "I AM SORRY TO HEAR YOU ARE SAD"
        );
    }

    #[test]
    fn it_should_reject_non_text_openai_responses_input() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/responses",
            &json!({
                "model": "eliza-doctor",
                "input": [{
                    "type": "message",
                    "role": "user",
                    "content": [{ "type": "input_image", "image_url": "https://example.com/image.png" }]
                }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["type"], "unsupported_request_error");
        assert_eq!(body["error"]["param"], "input.content");
    }

    #[test]
    fn it_should_bearer_auth_rejects_missing_token() {
        let server = TestServer::spawn(&["--auth", "bearer", "--bearer-token", "secret"]);
        let TestResponse { status, body } = server.post_json_with(
            "/v1/chat/completions",
            &json!({
                "model": "eliza-doctor",
                "messages": [{ "role": "user", "content": "Hello" }]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&body).expect("response should be JSON");
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body["error"]["type"], "authentication_error");
    }
}

#[test]
fn it_should_generate_openapi_from_typed_routes() {
    let server = TestServer::spawn(&[]);
    let TestResponse { status, body } = server.get("/openapi.json");
    let document: Value = serde_json::from_str(&body).expect("OpenAPI should be JSON");
    let paths = document["paths"]
        .as_object()
        .expect("OpenAPI should contain paths");
    let mut path_names = paths.keys().cloned().collect::<Vec<_>>();
    path_names.sort();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        path_names,
        [
            "/healthz",
            "/v1/chat/completions",
            "/v1/messages",
            "/v1/models",
            "/v1/responses",
            "/v1beta/models",
            "/v1beta/models/{model_action}",
            "/v1beta/openai/chat/completions",
        ]
    );
    assert!(document["paths"]["/v1/chat/completions"]["post"]["requestBody"].is_object());
    assert!(document["paths"]["/v1/messages"]["post"]["requestBody"].is_object());
    assert!(document["paths"]["/v1beta/models/{model_action}"]["post"]["requestBody"].is_object());
    assert!(document["components"]["schemas"].is_object());
}

/// Collect captured server diagnostics after the child has exited.
fn child_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        pipe.read_to_string(&mut stderr)
            .expect("server stderr should be readable");
    }
    stderr
}
