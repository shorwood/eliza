//! Liveness route and response contract.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;

// -----------------------------------------------------------------------------
// HealthResponse: Describes liveness state.
// -----------------------------------------------------------------------------

/// Liveness response returned by the system health route.
#[derive(Debug, Serialize, JsonSchema)]
struct HealthResponse {
    /// Stable health state for machine consumers.
    status: &'static str,
    /// Service identifier used by local orchestration.
    service: &'static str,
}

/// Report that the in-process HTTP service is alive.
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eliza",
    })
}

/// Build the documented liveness route.
pub(super) fn router() -> ApiRouter {
    ApiRouter::new().api_route(
        "/healthz",
        get_with(healthz, |operation| {
            operation.summary("Health check").tag("system")
        }),
    )
}
