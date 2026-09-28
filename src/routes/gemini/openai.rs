//! Gemini's `OpenAI`-compatible alias.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;

use crate::routes::context::AppState;
use crate::routes::openai::chat_completions::OpenAiChatCompletions;
use crate::routes::openai::types::{ChatCompletionResponse, OpenAiFailureResponse};

/// `OpenAI`-compatible route exposed below the Gemini namespace.
pub(super) struct OpenAiAlias;

impl OpenAiAlias {
    /// Mount Gemini's `OpenAI`-compatible Chat Completions route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/v1beta/openai/chat/completions",
            post_with(OpenAiChatCompletions::handle, |operation| {
                operation
                    .summary("Gemini OpenAI chat")
                    .tag("gemini")
                    .response::<200, Json<ChatCompletionResponse>>()
                    .default_response::<Json<OpenAiFailureResponse>>()
            }),
        )
    }
}
