// Tests: Verify provider compatibility routes and authentication.
//! End-to-end provider compatibility checks through the assembled router.

// -----------------------------------------------------------------------------

/// Checks ELIZA provider compatibility behavior at its source owner.
#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    /// Checks the anthropic contract.
    mod anthropic {
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_anthropic_message_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "messages": [{ "role": "user", "content": "I am sad" }]
                }),
                &[support::TestHeader {
                    name: "anthropic-version",
                    value: "2023-06-01",
                }],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["type"], "message");
            assert_eq!(body["content"][0]["type"], "text");
            assert_eq!(body["content"][0]["text"], "I AM SORRY TO HEAR YOU ARE SAD");
            assert_eq!(body["stop_reason"], "end_turn");
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_anthropic_stream_uses_named_sse_events() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/messages",
                json!({
                    "model": "eliza-doctor",
                    "max_tokens": 128,
                    "stream": true,
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[support::TestHeader {
                    name: "anthropic-version",
                    value: "2023-06-01",
                }],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("event: message_start"));
            assert!(body.contains("event: content_block_delta"));
            let has_message_stop = body.contains("event: message_stop");
            assert!(has_message_stop);
        }
    }
    /// Checks the gemini contract.
    mod gemini {
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_generate_content_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:generateContent",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": "I am sad" }]
                    }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["candidates"][0]["content"]["role"], "model");
            assert_eq!(
                body["candidates"][0]["content"]["parts"][0]["text"],
                "I AM SORRY TO HEAR YOU ARE SAD"
            );
            assert_eq!(body["candidates"][0]["finishReason"], "STOP");
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_stream_generate_content_can_return_sse() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:streamGenerateContent?alt=sse",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "text": "Hello" }]
                    }]
                }),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("data:"));
            let has_finish_reason = body.contains("finishReason");
            assert!(has_finish_reason);
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_rejects_multimodal_parts_with_gemini_error() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/models/eliza-doctor:generateContent",
                json!({
                    "contents": [{
                        "role": "user",
                        "parts": [{ "inlineData": { "mimeType": "image/png", "data": "AA==" } }]
                    }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
        }
    }
    /// Checks the openai contract.
    mod openai {
        use axum::http::StatusCode;
        use serde_json::{Value, json};

        use crate::cli::BearerToken;
        use crate::serve::ServerConfig;
        use crate::test_support as support;

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_chat_completion_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "I am sad" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["object"], "chat.completion");
            assert_eq!(
                body["choices"][0]["message"]["content"],
                "I AM SORRY TO HEAR YOU ARE SAD"
            );
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_streaming_chat_completion_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "stream": true,
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert!(body.contains("chat.completion.chunk"));
            let has_done_event = body.contains("data: [DONE]");
            assert!(has_done_event);
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_gemini_openai_alias_uses_openai_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1beta/openai/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            let object = &body["object"];
            assert_eq!(object, "chat.completion");
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_openai_responses_shape() {
            let support::TestResponse { status, body } = support::post_json_with(
                ServerConfig::default(),
                "/v1/responses",
                json!({
                    "model": "eliza-doctor",
                    "input": "I need help"
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["object"], "response");
            assert_eq!(body["output"][0]["content"][0]["type"], "output_text");
        }

        /// A change here must not break a supported provider wire shape.
        #[tokio::test]
        async fn it_should_bearer_auth_rejects_missing_token() {
            let config = ServerConfig::default()
                .with_bearer_token("secret".parse::<BearerToken>().expect("token should parse"));
            let support::TestResponse { status, body } = support::post_json_with(
                config,
                "/v1/chat/completions",
                json!({
                    "model": "eliza-doctor",
                    "messages": [{ "role": "user", "content": "Hello" }]
                }),
                &[],
            )
            .await;
            let body: Value = serde_json::from_str(&body).expect("response should be json");
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(body["error"]["type"], "authentication_error");
        }
    }
}
