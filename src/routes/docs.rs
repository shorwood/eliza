//! Generated `OpenAPI` and Scalar documentation routes.

use aide::axum::ApiRouter;
use aide::openapi::{Info, OpenApi};
use aide::scalar::Scalar;
use axum::routing::get;
use axum::{Extension, Json, Router};

use super::context::AppState;

// -----------------------------------------------------------------------------
// OpenapiJson: Serves the generated specification.
// -----------------------------------------------------------------------------

/// Returns the generated `OpenAPI` document mounted into the router.
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
                "A standalone ELIZA server exposing OpenAI, Anthropic, and Gemini-compatible endpoints."
                    .to_owned(),
            ),
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
        router
            .route("/openapi.json", get(openapi_json))
            .route("/docs", docs)
            .finish_api(&mut api)
            .layer(Extension(api))
    }
}

// -----------------------------------------------------------------------------
// Tests: Verify generated paths and typed request schemas.
// -----------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::missing_panics_doc,
    reason = "test rationales replace public panic contracts"
)]
mod tests {
    use axum::http::StatusCode;
    use serde_json::Value;

    use crate::serve::ServerConfig;
    use crate::test_support as support;

    /// Generated documentation reflects every typed provider route.
    #[tokio::test]
    async fn it_should_generate_openapi_from_typed_routes() {
        let support::TestResponse { status, body } =
            support::get(ServerConfig::default(), "/openapi.json").await;
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
                "/healthz",
                "/v1/chat/completions",
                "/v1/messages",
                "/v1/models",
                "/v1/responses",
                "/v1beta/models",
                "/v1beta/models/{model_action}",
                "/v1beta/openai/chat/completions",
            ]
        );
        assert!(document["paths"]["/v1/chat/completions"]["post"]["requestBody"].is_object());
        assert!(document["paths"]["/v1/messages"]["post"]["requestBody"].is_object());
        assert!(
            document["paths"]["/v1beta/models/{model_action}"]["post"]["requestBody"].is_object()
        );
        assert!(document["components"]["schemas"].is_object());
    }
}
