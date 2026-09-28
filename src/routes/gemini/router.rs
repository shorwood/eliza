//! Gemini route-tree assembly.

use aide::axum::ApiRouter;

use super::content::Route as ContentRoute;
use super::models::GeminiModels;
use super::openai::OpenAiAlias;
use crate::routes::context::AppState;

/// Complete Gemini-compatible route tree.
pub(crate) struct ProviderRoutes;

impl ProviderRoutes {
    /// Build the Gemini-compatible route tree.
    pub(crate) fn build() -> ApiRouter<AppState> {
        let router = GeminiModels::mount(ApiRouter::new());
        let router = ContentRoute::mount(router);
        OpenAiAlias::mount(router)
    }
}
