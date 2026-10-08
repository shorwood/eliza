//! Generated `OpenAPI` and Scalar documentation routes.

use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::openapi::{Info, License, OpenApi};
#[cfg(not(embedded_scalar))]
use aide::scalar::Scalar;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};

/// Pinned Scalar browser bundle supplied by the Nix build.
#[cfg(embedded_scalar)]
const SCALAR_JS: &str = include_str!(env!("ELIZA_SCALAR_JS"));

/// Reference page whose relative URLs work at both root and `/api` mounts.
#[cfg(embedded_scalar)]
const SCALAR_HTML: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>ELIZA API Reference</title><style>body{margin:0}</style></head>
<body>
<div id="app"></div>
<script src="./scalar.js"></script>
<script>Scalar.createApiReference('#app', { url: './openapi.json', theme: 'kepler', darkMode: true, withDefaultFonts: false })</script>
</body>
</html>"#;

/// Serve the embedded browser bundle with a JavaScript content type.
#[cfg(embedded_scalar)]
async fn scalar_js() -> Response {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        SCALAR_JS,
    )
        .into_response()
}

/// Serve the interactive reference document.
#[cfg(embedded_scalar)]
async fn scalar_html() -> axum::response::Html<&'static str> {
    axum::response::Html(SCALAR_HTML)
}

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
        #[cfg(not(embedded_scalar))]
        let docs = Scalar::new("/openapi.json")
            .with_title("ELIZA API")
            .axum_route();
        #[cfg(embedded_scalar)]
        let docs = get(scalar_html);
        let router = self
            .route("/openapi.json", get(openapi_json))
            .route("/docs", docs);
        #[cfg(embedded_scalar)]
        let router = router.route("/scalar.js", get(scalar_js));
        router.finish_api(&mut api).layer(Extension(Arc::new(api)))
    }
}
