//! API module for the bitservice peer.
//!
//! This module defines all HTTP endpoints exposed by the bitservice peer and organizes them into submodules:
//!
//! - [`errors`] – Defines API error types and conversions from internal service errors.
//! - [`health`] – Provides health endpoint (`/health`).
//! - [`v1`] – Version 1 of the main bitservice server endpoints, including `/read` and `/write`.

use axum::Router;
use tower_http::trace::TraceLayer;

use crate::AppState;

#[cfg(test)]
use axum_test::TestServer;

pub(crate) mod errors;
pub(crate) mod health;
pub(crate) mod v1;

/// Builds the main API router for the bitservice peer.
///
/// This function sets up:
///
/// - The `/api/v1` endpoints from [`v1`].
/// - The health and readiness endpoints from [`health`].
/// - An HTTP trace layer via [`TraceLayer`].
///
/// The returned [`Router`] has an [`AppState`] attached that contains the configuration and service
/// instances needed to handle requests.
pub(crate) fn new_app(app_state: AppState) -> Router {
    Router::new()
        .nest("/api/v1", v1::build())
        .merge(health::routes())
        .layer(TraceLayer::new_for_http())
        .with_state(app_state)
}

/// Builds a [`TestServer`] with the same configuration as [`new_app`].
///
/// This function is only compiled in tests (`#[cfg(test)]`) and provides a convenient way
/// to spin up the full API with mock services and expectations.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) fn new_test_app(app_state: AppState) -> TestServer {
    let app = new_app(app_state);
    TestServer::builder()
        .expect_success_by_default()
        .mock_transport()
        .build(app)
        .unwrap()
}
