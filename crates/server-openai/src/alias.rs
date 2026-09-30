//! Gemini's `OpenAI`-compatible alias.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use eliza_http::context::AppState;

use crate::chat_completions::OpenAiChatCompletions;
use crate::embeddings::{OpenAiEmbeddingResponse, OpenAiEmbeddings};
use crate::errors::OpenAiFailureResponse;
use crate::types::ChatResponse;

/// OpenAI-compatible routes exposed below Gemini's namespace.
pub struct GeminiAlias;

impl GeminiAlias {
    /// Mount the OpenAI-compatible aliases exposed by Gemini.
    pub fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        let router = router.api_route(
            "/v1beta/openai/chat/completions",
            post_with(OpenAiChatCompletions::handle, |operation| {
                operation
                    .summary("Gemini OpenAI chat")
                    .tag("gemini")
                    .response::<200, Json<ChatResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        );
        router.api_route(
            "/v1beta/openai/embeddings",
            post_with(OpenAiEmbeddings::handle, |operation| {
                operation
                    .summary("Gemini OpenAI embeddings")
                    .tag("gemini")
                    .response::<200, Json<OpenAiEmbeddingResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
