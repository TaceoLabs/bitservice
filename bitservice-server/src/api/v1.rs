//! Version 1 (v1) API Routes
//!
//! This module defines the v1 API routes for the bitservice server
//! Currently, all endpoints are unauthenticated.
//!
//! It also applies a restrictive CORS policy suitable for JSON-based POST requests.
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::post,
};
use bitservice_types::{
    ban::{BanRequest, BanResponse},
    read::{ReadRequest, ReadResponse},
    unban::{UnbanRequest, UnbanResponse},
};
use tracing::instrument;

use crate::{
    AppState,
    api::errors::{ApiErrors, ApiResult},
};

/// Build the v1 API router.
///
/// This sets up:
/// - a restrictive CORS layer allowing JSON POST requests and OPTIONS preflight and a wildcard origin.
pub(crate) fn build() -> Router<AppState> {
    // TODO
    // We setup a wildcard as we are a public API and everyone can access the service.
    // let cors = CorsLayer::new()
    //     .allow_credentials(false)
    //     .allow_headers([http::header::CONTENT_TYPE, http::header::USER_AGENT])
    //     .allow_methods([http::Method::POST, http::Method::OPTIONS])
    //     .allow_origin(AllowOrigin::any());
    Router::new()
        .route("/read/{rp_id}", post(read))
        .route("/ban/{rp_id}", post(ban))
        .route("/unban/{rp_id}", post(unban))
}

#[instrument(level = "debug", skip_all, fields(rp_id = rp_id))]
async fn read(
    State(state): State<AppState>,
    Path(rp_id): Path<u128>,
    Json(req): Json<ReadRequest>,
) -> ApiResult<Json<ReadResponse>> {
    tracing::debug!("received read request for rp {rp_id}");

    let rp_bitservice = state
        .rp_bitservices
        .get(&rp_id)
        .ok_or_else(|| ApiErrors::NotFound(format!("unknown rp_id: {rp_id}")))?;

    let res = rp_bitservice.rw_queue.read(req).await?;

    Ok(Json(res))
}

#[instrument(level = "debug", skip_all, fields(rp_id = rp_id))]
async fn ban(
    State(state): State<AppState>,
    Path(rp_id): Path<u128>,
    Json(req): Json<BanRequest>,
) -> ApiResult<Json<BanResponse>> {
    tracing::debug!("received ban request for rp {rp_id}");

    let rp_bitservice = state
        .rp_bitservices
        .get(&rp_id)
        .ok_or_else(|| ApiErrors::NotFound(format!("unknown rp_id: {rp_id}")))?;

    let res = rp_bitservice.rw_queue.ban(req).await?;

    Ok(Json(res))
}

#[instrument(level = "debug", skip_all, fields(rp_id = rp_id))]
async fn unban(
    State(state): State<AppState>,
    Path(rp_id): Path<u128>,
    Json(req): Json<UnbanRequest>,
) -> ApiResult<Json<UnbanResponse>> {
    tracing::debug!("received unban request for rp {rp_id}");

    let rp_bitservice = state
        .rp_bitservices
        .get(&rp_id)
        .ok_or_else(|| ApiErrors::NotFound(format!("unknown rp_id: {rp_id}")))?;

    let res = rp_bitservice.rw_queue.unban(req).await?;

    Ok(Json(res))
}
