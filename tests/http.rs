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

/// Return the shared Chat Completions and Ollama function definition.
fn openai_tool() -> Value {
    json!({
        "type":"function",
        "function":{
            "name":"echo",
            "description":"Echo a value",
            "parameters":{
                "type":"object",
                "properties":{"value":{"type":"string"}},
                "required":["value"]
            }
        }
    })
}

/// Return the shared Responses API function definition.
fn responses_tool() -> Value {
    json!({
        "type":"function",
        "name":"echo",
        "description":"Echo a value",
        "parameters":{
            "type":"object",
            "properties":{"value":{"type":"string"}},
            "required":["value"]
        }
    })
}

/// Return the shared Anthropic function definition.
fn anthropic_tool() -> Value {
    json!({
        "name":"echo",
        "description":"Echo a value",
        "input_schema":{
            "type":"object",
            "properties":{"value":{"type":"string"}},
            "required":["value"]
        }
    })
}

/// Return the shared native Gemini function declaration wrapper.
fn gemini_tools() -> Value {
    json!([{"functionDeclarations":[{
        "name":"echo",
        "description":"Echo a value",
        "parameters":{
            "type":"object",
            "properties":{"value":{"type":"string"}},
            "required":["value"]
        }
    }]}])
}

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

    /// Send one GET request with optional provider headers.
    fn get_with(&self, path: &str, headers: &[TestHeader<'_>]) -> TestResponse {
        let mut request = self.agent.get(format!("{}{path}", self.base_url));
        for header in headers {
            request = request.header(header.name, header.value);
        }
        let response = request.call().expect("GET request should complete");
        response.into()
    }

    /// Send one GET request to the spawned server.
    fn get(&self, path: &str) -> TestResponse {
        self.get_with(path, &[])
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
            "/anthropic/v1/messages",
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
            "/anthropic/v1/messages",
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

    #[test]
    fn it_should_round_trip_an_anthropic_tool_call() {
        let server = TestServer::spawn(&[]);
        let first = server.post_json_with(
            "/anthropic/v1/messages",
            &json!({
                "model":"eliza-doctor",
                "max_tokens":128,
                "tools":[anthropic_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        let call: Value = serde_json::from_str(&first.body).expect("tool call should be JSON");
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(call["content"][0]["type"], "tool_use");
        assert_eq!(call["content"][0]["input"], json!({"value":"hello"}));
        assert_eq!(call["stop_reason"], "tool_use");

        let second = server.post_json_with(
            "/anthropic/v1/messages",
            &json!({
                "model":"eliza-doctor",
                "max_tokens":128,
                "tools":[anthropic_tool()],
                "messages":[
                    {"role":"user","content":"@tool echo {\"value\":\"hello\"}"},
                    {"role":"assistant","content":call["content"].clone()},
                    {"role":"user","content":[{
                        "type":"tool_result",
                        "tool_use_id":call["content"][0]["id"],
                        "content":"hello"
                    }]}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&second.body).expect("result should be JSON");
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(body["content"][0]["text"], "TOOL CALL COMPLETE");
    }

    #[test]
    fn it_should_preserve_mixed_anthropic_user_block_order() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/anthropic/v1/messages",
            &json!({
                "model":"eliza-doctor",
                "max_tokens":128,
                "tools":[anthropic_tool()],
                "messages":[
                    {"role":"user","content":"@tool echo {\"value\":\"hello\"}"},
                    {"role":"assistant","content":[{
                        "type":"tool_use",
                        "id":"toolu_fixture",
                        "name":"echo",
                        "input":{"value":"hello"}
                    }]},
                    {"role":"user","content":[
                        {"type":"text","text":"I am sad"},
                        {"type":"tool_result","tool_use_id":"toolu_fixture","content":"hello"}
                    ]}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&response.body).expect("result should be JSON");
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(body["content"][0]["text"], "TOOL CALL COMPLETE");
    }

    #[test]
    fn it_should_stream_an_anthropic_tool_call() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/anthropic/v1/messages",
            &json!({
                "model":"eliza-doctor",
                "max_tokens":128,
                "stream":true,
                "tools":[anthropic_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        assert_eq!(response.status, StatusCode::OK);
        assert!(response.body.contains("input_json_delta"));
        assert!(response.body.contains("tool_use"));
    }
}

mod gemini {
    use super::*;

    #[test]
    fn it_should_gemini_generate_content_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/gemini/v1beta/models/eliza-doctor:generateContent",
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
            "/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse",
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
            "/gemini/v1beta/models/eliza-doctor:generateContent",
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

    #[test]
    fn it_should_round_trip_a_gemini_function_call() {
        let server = TestServer::spawn(&[]);
        let first = server.post_json_with(
            "/gemini/v1beta/models/eliza-doctor:generateContent",
            &json!({
                "tools":gemini_tools(),
                "contents":[{"role":"user","parts":[{
                    "text":"@tool echo {\"value\":\"hello\"}"
                }]}]
            }),
            &[],
        );
        let call: Value = serde_json::from_str(&first.body).expect("tool call should be JSON");
        let function = &call["candidates"][0]["content"]["parts"][0]["functionCall"];
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(function["name"], "echo");
        assert_eq!(function["args"], json!({"value":"hello"}));

        let second = server.post_json_with(
            "/gemini/v1beta/models/eliza-doctor:generateContent",
            &json!({
                "tools":gemini_tools(),
                "contents":[
                    {"role":"user","parts":[{
                        "text":"@tool echo {\"value\":\"hello\"}"
                    }]},
                    {"role":"model","parts":[{"functionCall":function.clone()}]},
                    {"role":"user","parts":[{"functionResponse":{
                        "name":"echo",
                        "response":{"value":"hello"}
                    }}]}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&second.body).expect("result should be JSON");
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(
            body["candidates"][0]["content"]["parts"][0]["text"],
            "TOOL CALL COMPLETE"
        );
    }

    #[test]
    fn it_should_stream_a_gemini_function_call() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/gemini/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse",
            &json!({
                "tools":gemini_tools(),
                "contents":[{"role":"user","parts":[{
                    "text":"@tool echo {\"value\":\"hello\"}"
                }]}]
            }),
            &[],
        );
        assert_eq!(response.status, StatusCode::OK);
        assert!(response.body.contains("functionCall"));
        assert!(response.body.contains("echo"));
    }
}

mod openai {
    use super::*;

    #[test]
    fn it_should_openai_chat_completion_shape() {
        let server = TestServer::spawn(&[]);
        let TestResponse { status, body } = server.post_json_with(
            "/openai/v1/chat/completions",
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
            "/openai/v1/chat/completions",
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
            "/gemini/v1beta/openai/chat/completions",
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
            "/openai/v1/responses",
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
            "/openai/v1/responses",
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
            "/openai/v1/responses",
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
    fn it_should_round_trip_an_openai_chat_tool_call() {
        let server = TestServer::spawn(&[]);
        let first = server.post_json_with(
            "/openai/v1/chat/completions",
            &json!({
                "model":"eliza-doctor",
                "tools":[openai_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        let call: Value = serde_json::from_str(&first.body).expect("tool call should be JSON");
        let tool_call = &call["choices"][0]["message"]["tool_calls"][0];
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(call["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(tool_call["function"]["name"], "echo");
        assert_eq!(tool_call["function"]["arguments"], "{\"value\":\"hello\"}");

        let second = server.post_json_with(
            "/openai/v1/chat/completions",
            &json!({
                "model":"eliza-doctor",
                "tools":[openai_tool()],
                "messages":[
                    {"role":"user","content":"@tool echo {\"value\":\"hello\"}"},
                    {"role":"assistant","content":null,"tool_calls":[tool_call.clone()]},
                    {"role":"tool","tool_call_id":tool_call["id"],"content":"hello"}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&second.body).expect("result should be JSON");
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(
            body["choices"][0]["message"]["content"],
            "TOOL CALL COMPLETE"
        );
    }

    #[test]
    fn it_should_stream_openai_tool_calls_and_usage() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/openai/v1/chat/completions",
            &json!({
                "model":"eliza-doctor",
                "stream":true,
                "stream_options":{"include_usage":true},
                "tools":[openai_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        assert_eq!(response.status, StatusCode::OK);
        assert!(response.body.contains("tool_calls"));
        assert!(response.body.contains("prompt_tokens"));
        assert!(response.body.contains("data: [DONE]"));
    }

    #[test]
    fn it_should_not_call_tools_without_the_explicit_marker() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/openai/v1/chat/completions",
            &json!({
                "model":"eliza-doctor",
                "tools":[openai_tool()],
                "messages":[{"role":"user","content":"What is the weather?"}]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&response.body).expect("response should be JSON");
        assert_eq!(response.status, StatusCode::OK);
        assert!(body["choices"][0]["message"]["tool_calls"].is_null());
        assert!(body["choices"][0]["message"]["content"].is_string());
    }

    #[test]
    fn it_should_round_trip_an_openai_responses_tool_call() {
        let server = TestServer::spawn(&[]);
        let first = server.post_json_with(
            "/openai/v1/responses",
            &json!({
                "model":"eliza-doctor",
                "tools":[responses_tool()],
                "input":"@tool echo {\"value\":\"hello\"}"
            }),
            &[],
        );
        let call: Value = serde_json::from_str(&first.body).expect("tool call should be JSON");
        let output = &call["output"][0];
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(output["type"], "function_call");
        assert_eq!(output["arguments"], "{\"value\":\"hello\"}");

        let second = server.post_json_with(
            "/openai/v1/responses",
            &json!({
                "model":"eliza-doctor",
                "tools":[responses_tool()],
                "input":[
                    {"type":"message","role":"user","content":[{
                        "type":"input_text","text":"@tool echo {\"value\":\"hello\"}"
                    }]},
                    output.clone(),
                    {"type":"function_call_output","call_id":output["call_id"],"output":"hello"}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&second.body).expect("result should be JSON");
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(body["output_text"], "TOOL CALL COMPLETE");
    }

    #[test]
    fn it_should_stream_openai_responses_text_and_tools() {
        let server = TestServer::spawn(&[]);
        let text = server.post_json_with(
            "/openai/v1/responses",
            &json!({"model":"eliza-doctor","stream":true,"input":"Hello"}),
            &[],
        );
        assert_eq!(text.status, StatusCode::OK);
        assert!(text.body.contains("response.output_text.delta"));
        assert!(text.body.contains("response.completed"));

        let tool = server.post_json_with(
            "/openai/v1/responses",
            &json!({
                "model":"eliza-doctor",
                "stream":true,
                "tools":[responses_tool()],
                "input":"@tool echo {\"value\":\"hello\"}"
            }),
            &[],
        );
        assert_eq!(tool.status, StatusCode::OK);
        assert!(tool.body.contains("response.function_call_arguments.delta"));
        assert!(tool.body.contains("response.function_call_arguments.done"));
    }

    #[test]
    fn it_should_bearer_auth_rejects_missing_token() {
        let server = TestServer::spawn(&["--auth", "bearer", "--bearer-token", "secret"]);
        let TestResponse { status, body } = server.post_json_with(
            "/openai/v1/chat/completions",
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

mod ollama {
    use super::*;

    #[test]
    fn it_should_list_models_and_stream_ndjson_by_default() {
        let server = TestServer::spawn(&[]);
        let tags = server.get("/ollama/api/tags");
        let tags_body: Value = serde_json::from_str(&tags.body).expect("tags should be JSON");
        assert_eq!(tags.status, StatusCode::OK);
        assert_eq!(tags_body["models"][0]["model"], "eliza-doctor");

        let response = server.post_json_with(
            "/ollama/api/chat",
            &json!({
                "model":"eliza-doctor",
                "messages":[{"role":"user","content":"Hello"}]
            }),
            &[],
        );
        let records = response
            .body
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("record should be JSON"))
            .collect::<Vec<_>>();
        assert_eq!(response.status, StatusCode::OK);
        assert!(records.len() > 1);
        assert_eq!(records.last().expect("terminal record")["done"], true);
        assert_eq!(
            records.last().expect("terminal record")["done_reason"],
            "stop"
        );
    }

    #[test]
    fn it_should_round_trip_an_ollama_tool_call() {
        let server = TestServer::spawn(&[]);
        let first = server.post_json_with(
            "/ollama/api/chat",
            &json!({
                "model":"eliza-doctor",
                "stream":false,
                "tools":[openai_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        let call: Value = serde_json::from_str(&first.body).expect("tool call should be JSON");
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(call["message"]["tool_calls"][0]["function"]["name"], "echo");
        assert_eq!(
            call["message"]["tool_calls"][0]["function"]["arguments"],
            json!({"value":"hello"})
        );

        let second = server.post_json_with(
            "/ollama/api/chat",
            &json!({
                "model":"eliza-doctor",
                "stream":false,
                "tools":[openai_tool()],
                "messages":[
                    {"role":"user","content":"@tool echo {\"value\":\"hello\"}"},
                    call["message"].clone(),
                    {"role":"tool","tool_name":"echo","content":"hello"}
                ]
            }),
            &[],
        );
        let body: Value = serde_json::from_str(&second.body).expect("result should be JSON");
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(body["message"]["content"], "TOOL CALL COMPLETE");
    }

    #[test]
    fn it_should_stream_an_ollama_tool_call_then_a_terminal_record() {
        let server = TestServer::spawn(&[]);
        let response = server.post_json_with(
            "/ollama/api/chat",
            &json!({
                "model":"eliza-doctor",
                "tools":[openai_tool()],
                "messages":[{"role":"user","content":"@tool echo {\"value\":\"hello\"}"}]
            }),
            &[],
        );
        let records = response
            .body
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("record should be JSON"))
            .collect::<Vec<_>>();
        assert_eq!(response.status, StatusCode::OK);
        assert_eq!(
            records[0]["message"]["tool_calls"][0]["function"]["name"],
            "echo"
        );
        assert_eq!(records.last().expect("terminal record")["done"], true);
    }
}

#[test]
fn it_should_expose_provider_native_model_catalogs() {
    let server = TestServer::spawn(&[]);
    for (path, pointer) in [
        ("/openai/v1/models", "/data/0/id"),
        ("/anthropic/v1/models", "/data/0/id"),
        ("/gemini/v1beta/models", "/models/0/name"),
        ("/ollama/api/tags", "/models/0/model"),
    ] {
        let response = server.get(path);
        let body: Value = serde_json::from_str(&response.body).expect("catalog should be JSON");
        assert_eq!(response.status, StatusCode::OK, "catalog {path}");
        assert!(body.pointer(pointer).is_some(), "catalog {path}");
    }
}

#[test]
fn it_should_authenticate_each_provider_with_its_native_header() {
    let server = TestServer::spawn(&["--auth", "bearer", "--bearer-token", "secret"]);
    let bearer = [TestHeader {
        name: "authorization",
        value: "Bearer secret",
    }];
    let anthropic = [TestHeader {
        name: "x-api-key",
        value: "secret",
    }];
    let gemini = [TestHeader {
        name: "x-goog-api-key",
        value: "secret",
    }];

    assert_eq!(
        server.get_with("/openai/v1/models", &bearer).status,
        StatusCode::OK
    );
    assert_eq!(
        server.get_with("/openai/v1/models", &anthropic).status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server.get_with("/anthropic/v1/models", &anthropic).status,
        StatusCode::OK
    );
    assert_eq!(
        server.get_with("/anthropic/v1/models", &bearer).status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        server.get_with("/gemini/v1beta/models", &gemini).status,
        StatusCode::OK
    );
    assert_eq!(
        server.get_with("/ollama/api/tags", &bearer).status,
        StatusCode::OK
    );
}

#[test]
fn it_should_not_mount_the_old_unprefixed_routes() {
    let server = TestServer::spawn(&[]);
    for path in ["/v1/models", "/v1/messages", "/v1beta/models"] {
        assert_eq!(server.get(path).status, StatusCode::NOT_FOUND, "old {path}");
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
            "/anthropic/v1/messages",
            "/anthropic/v1/models",
            "/gemini/v1beta/models",
            "/gemini/v1beta/models/{model_action}",
            "/gemini/v1beta/openai/chat/completions",
            "/healthz",
            "/ollama/api/chat",
            "/ollama/api/tags",
            "/openai/v1/chat/completions",
            "/openai/v1/models",
            "/openai/v1/responses",
        ]
    );
    assert!(document["paths"]["/openai/v1/chat/completions"]["post"]["requestBody"].is_object());
    assert!(document["paths"]["/anthropic/v1/messages"]["post"]["requestBody"].is_object());
    assert!(
        document["paths"]["/gemini/v1beta/models/{model_action}"]["post"]["requestBody"]
            .is_object()
    );
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
