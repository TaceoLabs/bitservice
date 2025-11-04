//! Version 1 (v1) API Routes
//!
//! This module defines the v1 API routes for the bitservice peer.
//! Currently, all endpoints are unauthenticated
//!
//! It also applies a restrictive CORS policy suitable for JSON-based POST requests.

use axum::{
    Json, Router,
    extract::{Path, State, WebSocketUpgrade},
    response::Response,
    routing::{any, post},
};
use bitservice_types::{
    ban::{PeerBanRequest, PeerBanResponse},
    prune::{PeerPruneRequest, PeerPruneResponse},
    read::{PeerReadRequest, PeerReadResponse},
    unban::{PeerUnbanRequest, PeerUnbanResponse},
};
use http::HeaderMap;
use tracing::instrument;
use uuid::Uuid;

use crate::{AppState, api::errors::ApiResult};

/// Build the v1 API router.
///
/// This sets up:
/// - a restrictive CORS layer allowing JSON POST requests and OPTIONS preflight and a wildcard origin.
pub(crate) fn build() -> Router<AppState> {
    // TODO
    // // We setup a wildcard as we are a public API and everyone can access the service.
    // let cors = CorsLayer::new()
    //     .allow_credentials(false)
    //     .allow_headers([http::header::CONTENT_TYPE, http::header::USER_AGENT])
    //     .allow_methods([http::Method::POST, http::Method::OPTIONS])
    //     .allow_origin(AllowOrigin::any());
    Router::new()
        .route("/read/{request_id}", post(read))
        .route("/ban/{request_id}", post(ban))
        .route("/unban/{request_id}", post(unban))
        .route("/prune/{request_id}", post(prune))
        .route("/ws", any(ws_handler))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn read(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(req): Json<PeerReadRequest>,
) -> ApiResult<Json<PeerReadResponse>> {
    tracing::debug!("received read request {request_id}");
    let res = state.ban_service.read(req, request_id).await?;
    tracing::debug!("handled read request {request_id}");
    Ok(Json(res))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn ban(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(req): Json<PeerBanRequest>,
) -> ApiResult<Json<PeerBanResponse>> {
    tracing::debug!("received ban request {request_id}");
    let res = state.ban_service.ban(req, request_id).await?;
    tracing::debug!("handled ban request {request_id}");
    Ok(Json(res))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn unban(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(req): Json<PeerUnbanRequest>,
) -> ApiResult<Json<PeerUnbanResponse>> {
    tracing::debug!("received unban request {request_id}");
    let res = state.ban_service.unban(req, request_id).await?;
    tracing::debug!("handled unban request {request_id}");
    Ok(Json(res))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn prune(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(_req): Json<PeerPruneRequest>,
) -> ApiResult<Json<PeerPruneResponse>> {
    tracing::debug!("received prune request {request_id}");
    state.ban_service.prune(request_id).await?;
    tracing::debug!("handled prune request {request_id}");
    Ok(Json(PeerPruneResponse {}))
}

/// The handler for the HTTP request (this gets called when the HTTP request lands at the start
/// of websocket negotiation). After this completes, the actual switching from HTTP to
/// websocket protocol will occur.
/// This is the last point where we can extract TCP/IP metadata such as IP address of the client
/// as well as things from HTTP headers such as user-agent of the browser etc.
#[instrument(level = "debug", skip_all)]
async fn ws_handler(
    headers: HeaderMap,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> axum::response::Result<Response> {
    state
        .ban_service
        .ws_sessions
        .handle_ws_request(headers, ws)
        .await
}
