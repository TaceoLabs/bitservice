//! Version 1 (v1) API Routes
//!
//! This module defines the v1 API routes for the bitservice peer.
//! Currently, all endpoints are unauthenticated
//!
//! It also applies a restrictive CORS policy suitable for JSON-based POST requests.
use std::{
    net::SocketAddr,
    str::FromStr,
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{State, WebSocketUpgrade},
    response::Response,
    routing::{any, post},
};
use bitservice_types::{
    PeerPruneRequest, PeerPruneResponse, PeerReadRequest, PeerReadResponse, PeerWriteRequest,
    PeerWriteResponse,
};
use eyre::Context;
use http::{HeaderMap, HeaderValue};
use mpc_core::protocols::rep3::id::PartyID;
use mpc_net::Network;
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, tungstenite::client::IntoClientRequest};
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;

use crate::{
    AppState, PendingTcpStreams, PendingWebsockets,
    api::errors::{ApiErrors, ApiResult},
    tcp_mpc_net::TcpNetwork,
    ws_mpc_net::WebSocketNetwork,
};

const REQUEST_ID_HEADER: &str = "request_id";
const STREAM_ID_HEADER: &str = "stream_id";
const MPC_NET_STREAM_0: u8 = 0;
const MPC_NET_STREAM_1: u8 = 1;

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
        .route("/read", post(read))
        .route("/write", post(write))
        .route("/prune", post(prune))
        .route("/ws", any(ws_handler))
}

#[instrument(level = "debug", skip_all, fields(request_id = %req.request_id))]
async fn read(
    State(state): State<AppState>,
    Json(req): Json<PeerReadRequest>,
) -> ApiResult<Json<PeerReadResponse>> {
    let PeerReadRequest { request_id } = req;
    let party_id = state.party_id;
    tracing::debug!("received read request {request_id}");

    let cancellation_token = CancellationToken::new();
    let (ws_net0, _net1) = init_ws_mpc_nets(
        state.pending_websockets,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;
    let (tcp_net0, _net1) = init_tcp_mpc_nets(
        state.pending_tcp_streams,
        cancellation_token.clone(),
        state.tcp_mpc_net_next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    tokio::task::block_in_place(move || {
        let msg = [0; 1000];
        let n = 10000;
        let start = Instant::now();
        for _ in 0..n {
            ws_net0.send(party_id.next().into(), &msg)?;
            let data = ws_net0.recv(party_id.prev().into())?;
            assert_eq!(data, msg, "is correct");
        }
        let duration = start.elapsed();
        let stats = ws_net0.get_connection_stats();
        tracing::info!("ws stats = {stats} took = {duration:?}");
        let start = Instant::now();
        for _ in 0..n {
            tcp_net0.send(party_id.next().into(), &msg)?;
            let data = tcp_net0.recv(party_id.prev().into())?;
            assert_eq!(data, msg, "is correct");
        }
        let duration = start.elapsed();
        let stats = tcp_net0.get_connection_stats();
        tracing::info!("tcp stats = {stats} took = {duration:?}");
        eyre::Ok(())
    })?;

    cancellation_token.cancel();

    Ok(Json(PeerReadResponse {}))
}

#[instrument(level = "debug", skip_all, fields(request_id = %req.request_id))]
async fn write(
    State(state): State<AppState>,
    Json(req): Json<PeerWriteRequest>,
) -> ApiResult<Json<PeerWriteResponse>> {
    let PeerWriteRequest { request_id } = req;
    let party_id = state.party_id;
    tracing::debug!("received write request {request_id}");

    let cancellation_token = CancellationToken::new();
    let (_net0, _net1) = init_ws_mpc_nets(
        state.pending_websockets,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    cancellation_token.cancel();

    Ok(Json(PeerWriteResponse {}))
}

#[instrument(level = "debug", skip_all, fields(request_id = %req.request_id))]
async fn prune(
    State(state): State<AppState>,
    Json(req): Json<PeerPruneRequest>,
) -> ApiResult<Json<PeerPruneResponse>> {
    let PeerPruneRequest { request_id } = req;
    let party_id = state.party_id;
    tracing::debug!("received prune request {request_id}");

    let cancellation_token = CancellationToken::new();
    let (_net0, _net1) = init_ws_mpc_nets(
        state.pending_websockets,
        cancellation_token.clone(),
        &state.next_peer,
        request_id,
        party_id,
        state.prev_peer_wait_timeout,
    )
    .await?;

    cancellation_token.cancel();

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
) -> ApiResult<Response> {
    let request_id = Uuid::from_str(
        headers
            .get(REQUEST_ID_HEADER)
            .ok_or_else(|| {
                ApiErrors::BadRequest(format!("missing header \"{REQUEST_ID_HEADER}\""))
            })?
            .to_str()
            .map_err(|_| {
                ApiErrors::BadRequest(format!("invalid header value for \"{REQUEST_ID_HEADER}\""))
            })?,
    )
    .map_err(|_| {
        ApiErrors::BadRequest(format!("invalid header value for \"{REQUEST_ID_HEADER}\""))
    })?;
    let stream_id = u8::from_str(
        headers
            .get(STREAM_ID_HEADER)
            .ok_or_else(|| ApiErrors::BadRequest(format!("missing header \"{STREAM_ID_HEADER}\"")))?
            .to_str()
            .map_err(|_| {
                ApiErrors::BadRequest(format!("invalid header value for \"{STREAM_ID_HEADER}\""))
            })?,
    )
    .map_err(|_| {
        ApiErrors::BadRequest(format!("invalid header value for \"{STREAM_ID_HEADER}\""))
    })?;
    tracing::info!("ws upgrade for request {request_id} stream {stream_id}");
    // finalize the upgrade process by returning upgrade callback.
    // we can customize the callback by sending additional info such as address.
    let response = ws.on_upgrade(move |socket| async move {
        if let Err(err) = state
            .pending_websockets
            .insert(request_id, stream_id, socket)
            .await
        {
            tracing::warn!("failed to insert pending websocket: {err:?}");
        }
    });
    Ok(response)
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id, party_id = %party_id))]
async fn init_tcp_mpc_nets(
    pending_tcp_streams: PendingTcpStreams,
    cancellation_token: CancellationToken,
    next_peer: SocketAddr,
    request_id: Uuid,
    party_id: PartyID,
    prev_peer_wait_timeout: Duration,
) -> eyre::Result<(TcpNetwork, TcpNetwork)> {
    tracing::debug!("connecting to next_peer: {next_peer}");
    let (next_stream0, next_stream1) = tokio::join!(
        tcp_connect_next(next_peer, request_id, MPC_NET_STREAM_0),
        tcp_connect_next(next_peer, request_id, MPC_NET_STREAM_1)
    );
    let next_stream0 = next_stream0?;
    let next_stream1 = next_stream1?;

    tracing::debug!("waiting for prev_peer");
    let (prev_stream0, prev_stream1) = tokio::time::timeout(prev_peer_wait_timeout, async {
        let prev_stream0 = pending_tcp_streams
            .get(request_id, MPC_NET_STREAM_0)
            .await?;
        let prev_stream1 = pending_tcp_streams
            .get(request_id, MPC_NET_STREAM_1)
            .await?;
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
    Ok((net0, net1))
}

#[instrument(level = "debug")]
async fn tcp_connect_next(
    addr: SocketAddr,
    request_id: Uuid,
    stream_id: u8,
) -> eyre::Result<TcpStream> {
    tracing::debug!("connecting to peer: {addr}");

    let mut stream = TcpStream::connect(addr).await?;

    stream.set_nodelay(true)?;
    stream.write_all(&request_id.into_bytes()).await?;
    stream.write_u8(stream_id).await?;

    tracing::debug!("connected");
    Ok(stream)
}

#[instrument(level = "debug", skip_all, fields(request_id = %request_id, party_id = %party_id))]
async fn init_ws_mpc_nets(
    pending_websockets: PendingWebsockets,
    cancellation_token: CancellationToken,
    next_peer: &str,
    request_id: Uuid,
    party_id: PartyID,
    prev_peer_wait_timeout: Duration,
) -> eyre::Result<(WebSocketNetwork, WebSocketNetwork)> {
    tracing::debug!("connecting to next_peer: {next_peer}");
    let (next_websocket0, next_websocket1) = tokio::join!(
        ws_connect_next(next_peer, request_id, MPC_NET_STREAM_0),
        ws_connect_next(next_peer, request_id, MPC_NET_STREAM_1)
    );
    let next_websocket0 = next_websocket0?;
    let next_websocket1 = next_websocket1?;

    tracing::debug!("waiting for prev_peer");
    let (prev_websocket0, prev_websocket1) = tokio::time::timeout(prev_peer_wait_timeout, async {
        let prev_websocket0 = pending_websockets.get(request_id, MPC_NET_STREAM_0).await?;
        let prev_websocket1 = pending_websockets.get(request_id, MPC_NET_STREAM_1).await?;
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

#[instrument(level = "debug")]
async fn ws_connect_next(
    ws_url: &str,
    request_id: Uuid,
    stream_id: u8,
) -> eyre::Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
    let mut request = ws_url
        .into_client_request()
        .context("while creating ws request")?;
    request.headers_mut().insert(
        REQUEST_ID_HEADER,
        HeaderValue::from_str(&request_id.to_string()).expect("can convert Uuid to HeaderValue"),
    );
    request.headers_mut().insert(
        STREAM_ID_HEADER,
        HeaderValue::from_str(&stream_id.to_string()).expect("can convert u8 to HeaderValue"),
    );
    tracing::debug!("connecting to peer: {ws_url}");

    let (websocket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("while connecting to websocket")?;
    tracing::debug!("connected");

    websocket.get_ref().get_ref().set_nodelay(true)?;
    Ok(websocket)
}
