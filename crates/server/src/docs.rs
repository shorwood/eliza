//! Generated `OpenAPI` and Scalar documentation routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::openapi::{Info, License, OpenApi};
use aide::scalar::Scalar;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};

// -----------------------------------------------------------------------------
// OpenapiJson: Serves the generated specification.
// -----------------------------------------------------------------------------

/// Return the generated `OpenAPI` document mounted into the router.
async fn openapi_json(Extension(api): Extension<Arc<OpenApi>>) -> Response {
    Json(api.as_ref()).into_response()
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
// ApiDocs: Finalizes and mounts generated documentation.
// -----------------------------------------------------------------------------

/// Finalization behavior for the complete documented API router.
pub(super) trait ApiDocs {
    /// Generate the specification and mount its JSON and Scalar viewers.
    fn finish_docs(self) -> Router;
}

impl ApiDocs for ApiRouter {
    fn finish_docs(self) -> Router {
        let mut api = document();
        let docs = Scalar::new("/openapi.json")
            .with_title("ELIZA API")
            .axum_route();
        let router = self
            .route("/openapi.json", get(openapi_json))
            .route("/docs", docs);
        router.finish_api(&mut api).layer(Extension(Arc::new(api)))
    }
}
