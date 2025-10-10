//! Version 1 (v1) API Routes
//!
//! This module defines the v1 API routes for the bitservice peer.
//! Currently, all endpoints are unauthenticated
//!
//! It also applies a restrictive CORS policy suitable for JSON-based POST requests.
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

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
use mpc_core::protocols::rep3::{Rep3State, conversion::A2BType, id::PartyID};
use oblivious_linear_scan_map::{ObliviousReadRequest, ObliviousUpdateRequest};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde::de::DeserializeOwned;
use tcp_mpc_net::{TcpNetwork, TcpSessions};
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;
use ws_mpc_net::{WebSocketNetwork, WsSessions};

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
    let party_id = state.party_id;
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let r = decode_unseal_deser(&state.crypto_device, &req.r, "r")?;

    let cancellation_token = CancellationToken::new();
    let (net0, net1) = init_ws_mpc_nets(
        state.ws_sessions,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    let oblivious_map = state.oblivious_map.read().await;

    let start = Instant::now();
    let res = tokio::task::block_in_place(|| {
        let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
        oblivious_map.oblivious_read(
            ObliviousReadRequest {
                key,
                randomness_commitment: r,
            },
            &net0,
            &net1,
            &mut rep3_state,
            &state.read_groth16,
        )
    })?;
    tracing::debug!("read took {:?}", start.elapsed());

    cancellation_token.cancel();

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
    let party_id = state.party_id;
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let value = decode_unseal_deser(&state.crypto_device, &req.value, "value")?;
    let r_key = decode_unseal_deser(&state.crypto_device, &req.r_key, "r_key")?;
    let r_value = decode_unseal_deser(&state.crypto_device, &req.r_value, "r_value")?;

    let cancellation_token = CancellationToken::new();
    let (net0, net1) = init_ws_mpc_nets(
        state.ws_sessions,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    let mut oblivious_map = state.oblivious_map.write().await;

    let start = Instant::now();
    let res = tokio::task::block_in_place(|| {
        let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
        oblivious_map.oblivious_insert_or_update(
            ObliviousUpdateRequest {
                key,
                update_value: value,
                randomness_index: r_key,
                randomness_commitment: r_value,
            },
            &net0,
            &net1,
            &mut rep3_state,
            &state.write_groth16,
        )
    })?;
    tracing::debug!("ban took {:?}", start.elapsed());

    cancellation_token.cancel();

    tracing::debug!("store map in db");
    state.db.store_map(&oblivious_map).await?;

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
    let party_id = state.party_id;
    let key = decode_unseal_deser(&state.crypto_device, &req.key, "key")?;
    let value = decode_unseal_deser(&state.crypto_device, &req.value, "value")?;
    let r_key = decode_unseal_deser(&state.crypto_device, &req.r_key, "r_key")?;
    let r_value = decode_unseal_deser(&state.crypto_device, &req.r_value, "r_value")?;

    let cancellation_token = CancellationToken::new();
    let (net0, net1) = init_ws_mpc_nets(
        state.ws_sessions,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    let mut oblivious_map = state.oblivious_map.write().await;

    let start = Instant::now();
    let res = tokio::task::block_in_place(|| {
        let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
        oblivious_map.oblivious_update(
            ObliviousUpdateRequest {
                key,
                update_value: value,
                randomness_index: r_key,
                randomness_commitment: r_value,
            },
            &net0,
            &net1,
            &mut rep3_state,
            &state.write_groth16,
        )
    })?;
    tracing::debug!("unban took {:?}", start.elapsed());

    cancellation_token.cancel();

    tracing::debug!("store map in db");
    state.db.store_map(&oblivious_map).await?;

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
    State(state): State<AppState>,
    Path(request_id): Path<Uuid>,
    Json(_req): Json<PeerPruneRequest>,
) -> ApiResult<Json<PeerPruneResponse>> {
    tracing::debug!("received prune request {request_id}");
    let party_id = state.party_id;

    let mut oblivious_map = state.oblivious_map.write().await;

    let cancellation_token = CancellationToken::new();
    let (_net0, _net1) = init_ws_mpc_nets(
        state.ws_sessions,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    let start = Instant::now();
    // let res = tokio::task::block_in_place(|| {
    //     let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
    //     todo!()
    // })?;
    tracing::debug!("prune took {:?}", start.elapsed());

    cancellation_token.cancel();

    tracing::debug!("store map in db");
    state.db.store_map(&oblivious_map).await?;

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
    ws_mpc_net::handle_ws_request(state.ws_sessions, headers, ws).await
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id, party_id = %party_id))]
async fn init_tcp_mpc_nets(
    tcp_sessions: TcpSessions,
    cancellation_token: CancellationToken,
    next_peer: SocketAddr,
    request_id: Uuid,
    party_id: PartyID,
    prev_peer_wait_timeout: Duration,
) -> eyre::Result<(TcpNetwork, TcpNetwork)> {
    tracing::debug!("connecting to next_peer: {next_peer}");

    let mut session_seed = [0u8; 32];
    session_seed[..16].copy_from_slice(&request_id.into_bytes());
    let mut session_rng = StdRng::from_seed(session_seed);
    let session0 = Uuid::from_bytes(session_rng.r#gen());
    let session1 = Uuid::from_bytes(session_rng.r#gen());

    let (next_stream0, next_stream1) = tokio::join!(
        tcp_mpc_net::tcp_connect(next_peer, session0),
        tcp_mpc_net::tcp_connect(next_peer, session1)
    );
    let next_stream0 = next_stream0?;
    let next_stream1 = next_stream1?;

    tracing::debug!("waiting for prev_peer");
    let (prev_stream0, prev_stream1) = tokio::time::timeout(prev_peer_wait_timeout, async {
        let prev_stream0 = tcp_sessions.get(session0).await?;
        let prev_stream1 = tcp_sessions.get(session1).await?;
        eyre::Ok((prev_stream0, prev_stream1))
    })
    .await??;

    tracing::debug!("creating mpc networks");
    let net0 = TcpNetwork::new(
        party_id,
        next_stream0,
        prev_stream0,
        cancellation_token.clone(),
    )?;
    let net1 = TcpNetwork::new(
        party_id,
        next_stream1,
        prev_stream1,
        cancellation_token.clone(),
    )?;
    tracing::debug!("done");
    Ok((net0, net1))
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id, party_id = %party_id))]
async fn init_ws_mpc_nets(
    ws_sessions: WsSessions,
    cancellation_token: CancellationToken,
    next_peer: &str,
    request_id: Uuid,
    party_id: PartyID,
    prev_peer_wait_timeout: Duration,
) -> eyre::Result<(WebSocketNetwork, WebSocketNetwork)> {
    tracing::debug!("connecting to next_peer: {next_peer}");

    let mut session_seed = [0u8; 32];
    session_seed[..16].copy_from_slice(&request_id.into_bytes());
    let mut session_rng = StdRng::from_seed(session_seed);
    let session0 = Uuid::from_bytes(session_rng.r#gen());
    let session1 = Uuid::from_bytes(session_rng.r#gen());

    let (next_websocket0, next_websocket1) = tokio::join!(
        ws_mpc_net::ws_connect(next_peer, session0),
        ws_mpc_net::ws_connect(next_peer, session1),
    );
    let next_websocket0 = next_websocket0?;
    let next_websocket1 = next_websocket1?;

    tracing::debug!("waiting for prev_peer");
    let (prev_websocket0, prev_websocket1) = tokio::time::timeout(prev_peer_wait_timeout, async {
        let prev_websocket0 = ws_sessions.get(session0).await?;
        let prev_websocket1 = ws_sessions.get(session1).await?;
        eyre::Ok((prev_websocket0, prev_websocket1))
    })
    .await??;

    tracing::debug!("creating mpc networks");
    let net0 = WebSocketNetwork::new(
        party_id,
        next_websocket0,
        prev_websocket0,
        cancellation_token.clone(),
    )?;
    let net1 = WebSocketNetwork::new(
        party_id,
        next_websocket1,
        prev_websocket1,
        cancellation_token.clone(),
    )?;
    Ok((net0, net1))
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
