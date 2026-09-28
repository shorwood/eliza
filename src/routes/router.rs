//! Assembly of provider-compatible routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use axum::Router;

use super::anthropic_messages::AnthropicMessages;
use super::anthropic_models::AnthropicModels;
use super::context::{AppState, RouteConfig};
use super::docs::Docs;
use super::gemini_content::Route as GeminiContentRoute;
use super::gemini_models::GeminiModels;
use super::health::Health;
use super::ollama::Ollama;
use super::openai_chat_completions::OpenAiChatCompletions;
use super::openai_models::OpenAiModels;
use super::openai_responses::OpenAiResponses;

/// Complete provider-compatible HTTP surface.
pub(crate) struct Routes;

impl Routes {
    /// Build every provider route and generate its `OpenAPI` document.
    pub(crate) fn build(config: RouteConfig) -> Router {
        let router = Health::mount(ApiRouter::new());
        let router = OpenAiModels::mount(router);
        let router = OpenAiChatCompletions::mount(router);
        let router = OpenAiResponses::mount(router);
        let router = AnthropicMessages::mount(router);
        let router = AnthropicModels::mount(router);
        let router = GeminiModels::mount(router);
        let router = GeminiContentRoute::mount(router);
        let router = Ollama::mount(router);
        Docs::finish(router).with_state(AppState {
            config: Arc::new(config),
        })
    }
}
