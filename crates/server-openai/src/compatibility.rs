//! Reusable OpenAI-compatible routes for provider aliases.

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use axum::Json;
use axum::extract::DefaultBodyLimit;
use eliza_modality_image as image;

use crate::chat_completions;
use crate::context::AppState;
use crate::embeddings::{self, OpenAiEmbeddingResponse};
use crate::errors::OpenAiFailureResponse;
use crate::types::ChatResponse;

/// Mount provider-neutral OpenAI-compatible aliases.
pub(super) fn router() -> ApiRouter<AppState> {
    let router = ApiRouter::new().api_route(
        "/chat/completions",
        post_with(chat_completions::handle, |operation| {
            operation
                .summary("OpenAI-compatible chat completion")
                .tag("openai")
                .response::<200, Json<ChatResponse>>()
                .response::<429, Json<OpenAiFailureResponse>>()
                .response::<503, Json<OpenAiFailureResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        })
        .layer(DefaultBodyLimit::max(image::limits::LIMIT_JSON_BODY)),
    );
    router.api_route(
        "/embeddings",
        post_with(embeddings::handle, |operation| {
            operation
                .summary("OpenAI-compatible embeddings")
                .tag("openai")
                .response::<200, Json<OpenAiEmbeddingResponse>>()
                .response::<429, Json<OpenAiFailureResponse>>()
                .response::<503, Json<OpenAiFailureResponse>>()
                .default_response::<Json<OpenAiFailureResponse>>()
        }),
    )
}
