//! Gemini's `OpenAI`-compatible alias.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;

use crate::routes::context::AppState;
use crate::routes::openai::chat_completions::OpenAiChatCompletions;
use crate::routes::openai::embeddings::{OpenAiEmbeddingResponse, OpenAiEmbeddings};
use crate::routes::openai::errors::OpenAiFailureResponse;
use crate::routes::openai::types::ChatResponse;

/// `OpenAI`-compatible route exposed below the Gemini namespace.
pub(super) struct OpenAiAlias;

impl OpenAiAlias {
    /// Mount Gemini's `OpenAI`-compatible Chat Completions route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
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
