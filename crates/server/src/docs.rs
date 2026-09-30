//! Generated `OpenAPI` and Scalar documentation routes.

use aide::axum::ApiRouter;
use aide::openapi::{Info, License, OpenApi};
use aide::scalar::Scalar;
use axum::routing::get;
use axum::{Extension, Json, Router};
use eliza_http::context::AppState;

// -----------------------------------------------------------------------------
// OpenapiJson: Serves the generated specification.
// -----------------------------------------------------------------------------

/// Return the generated `OpenAPI` document mounted into the router.
async fn openapi_json(Extension(api): Extension<OpenApi>) -> Json<OpenApi> {
    Json(api)
}

// -----------------------------------------------------------------------------
// Document: Defines API metadata.
// -----------------------------------------------------------------------------

/// Build metadata shared by the JSON and Scalar documentation routes.
fn document() -> OpenApi {
    OpenApi {
        info: Info {
            title: "ELIZA Compatibility Server".to_owned(),
            summary: Some("Classic ELIZA served through provider-compatible HTTP APIs.".to_owned()),
            description: Some(
                "A standalone ELIZA server exposing OpenAI, Anthropic, Gemini, and Ollama-compatible endpoints."
                    .to_owned(),
            ),
            license: Some(License {
                name: "MIT License".to_owned(),
                identifier: Some("MIT".to_owned()),
                ..License::default()
            }),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            ..Info::default()
        },
        ..OpenApi::default()
    }
}

// -----------------------------------------------------------------------------
// Docs: Mounts specification and viewer routes.
// -----------------------------------------------------------------------------

/// Generated `OpenAPI` and Scalar documentation endpoints.
pub(super) struct Docs;

impl Docs {
    /// Finish API generation and mount the generated document viewers.
    pub(super) fn finish(router: ApiRouter<AppState>) -> Router<AppState> {
        let mut api = document();
        let docs = Scalar::new("/openapi.json")
            .with_title("ELIZA API")
            .axum_route();
        let router = router
            .route("/openapi.json", get(openapi_json))
            .route("/docs", docs);
        router.finish_api(&mut api).layer(Extension(api))
    }
}
