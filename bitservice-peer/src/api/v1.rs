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
use base64::{Engine as _, engine::general_purpose::STANDARD};
use bitservice_types::{
    ban::{PeerBanRequest, PeerBanResponse},
    prune::{PeerPruneRequest, PeerPruneResponse},
    read::{PeerReadRequest, PeerReadResponse},
};
use http::HeaderMap;
use oblivious_linear_scan_map::{ObliviousReadRequest, ObliviousUpdateRequest};
use serde::de::DeserializeOwned;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    AppState,
    api::errors::{ApiErrors, ApiResult},
    crypto_device::CryptoDevice,
};

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
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let r = decode_unseal_deser(&state.crypto_device, &req.r, "r")?;

    let res = state
        .ban_service
        .read(
            ObliviousReadRequest {
                key,
                randomness_commitment: r,
            },
            request_id,
        )
        .await?;

    tracing::debug!("handled read request {request_id}");

    Ok(Json(PeerReadResponse {
        value: res.read,
        proof: res.proof.into(),
        root: res.root,
        commitment: res.commitment,
    }))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn ban(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(req): Json<PeerBanRequest>,
) -> ApiResult<Json<PeerBanResponse>> {
    tracing::debug!("received ban request {request_id}");
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let value = decode_unseal_deser(&state.crypto_device, &req.value, "value")?;
    let r_key = decode_unseal_deser(&state.crypto_device, &req.r_key, "r_key")?;
    let r_value = decode_unseal_deser(&state.crypto_device, &req.r_value, "r_value")?;

    let res = state
        .ban_service
        .ban(
            ObliviousUpdateRequest {
                key,
                update_value: value,
                randomness_index: r_key,
                randomness_commitment: r_value,
            },
            request_id,
        )
        .await?;

    tracing::debug!("handled ban request {request_id}");

    Ok(Json(PeerBanResponse {
        proof: res.proof.into(),
        old_root: res.old_root,
        new_root: res.new_root,
        commitment_key: res.commitment_key,
        commitment_value: res.commitment_value,
    }))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn unban(
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(req): Json<PeerBanRequest>,
) -> ApiResult<Json<PeerBanResponse>> {
    tracing::debug!("received unban request {request_id}");
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let value = decode_unseal_deser(&state.crypto_device, &req.value, "value")?;
    let r_key = decode_unseal_deser(&state.crypto_device, &req.r_key, "r_key")?;
    let r_value = decode_unseal_deser(&state.crypto_device, &req.r_value, "r_value")?;

    let res = state
        .ban_service
        .unban(
            ObliviousUpdateRequest {
                key,
                update_value: value,
                randomness_index: r_key,
                randomness_commitment: r_value,
            },
            request_id,
        )
        .await?;

    tracing::debug!("handled unban request {request_id}");

    Ok(Json(PeerBanResponse {
        proof: res.proof.into(),
        old_root: res.old_root,
        new_root: res.new_root,
        commitment_key: res.commitment_key,
        commitment_value: res.commitment_value,
    }))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
async fn prune(
    State(_state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(_req): Json<PeerPruneRequest>,
) -> ApiResult<Json<PeerPruneResponse>> {
    tracing::debug!("received prune request {request_id}");

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

fn decode_unseal_deser<T: DeserializeOwned>(
    crypto_device: &CryptoDevice,
    base64: &str,
    field: &str,
) -> ApiResult<T> {
    let ciphertext = STANDARD
        .decode(base64)
        .map_err(|_| ApiErrors::BadRequest(format!("invalid \"{field}\" base64")))?;
    let bytes = crypto_device
        .unseal(&ciphertext)
        .map_err(|_| ApiErrors::BadRequest(format!("invalid \"{field}\" ciphertext")))?;
    let (value, _) = bincode::serde::decode_from_slice::<T, _>(&bytes, bincode::config::standard())
        .map_err(|_| ApiErrors::BadRequest(format!("invalid \"{field}\" share bytes")))?;
    Ok(value)
}
