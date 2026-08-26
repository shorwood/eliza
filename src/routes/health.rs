//! Liveness route and response contract.

use aide::axum::ApiRouter;
use aide::axum::routing::get_with;
use axum::Json;
use schemars::JsonSchema;
use serde::Serialize;

use super::context::AppState;

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

/// Reports that the in-process HTTP service is alive.
async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "eliza",
    })
}

// -----------------------------------------------------------------------------
// Health: Mounts the liveness endpoint.
// -----------------------------------------------------------------------------

/// Liveness endpoint and its documentation contract.
pub(super) struct Health;

impl Health {
    /// Mount the documented liveness route.
    pub(super) fn mount(router: ApiRouter<AppState>) -> ApiRouter<AppState> {
        router.api_route(
            "/healthz",
            get_with(healthz, |operation| {
                operation.summary("Health check").tag("system")
            }),
        )
    }
}
