use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    extract::ws::{Message, WebSocket},
    serve::ListenerExt as _,
};
use futures::{
    StreamExt as _,
    stream::{SplitSink, SplitStream},
};
use mpc_core::protocols::rep3::id::PartyID;
use tokio::{
    io::AsyncReadExt,
    net::TcpStream,
    sync::{Mutex, oneshot},
};
use uuid::Uuid;

use crate::config::BitservicePeerConfig;

pub(crate) mod api;
pub mod config;
pub mod metrics;
pub(crate) mod tcp_mpc_net;
pub(crate) mod ws_mpc_net;

pub(crate) struct ServerWebSocket {
    pub(crate) sender: SplitSink<WebSocket, Message>,
    pub(crate) receiver: SplitStream<WebSocket>,
}

pub(crate) enum MaybeWebSocket {
    WebSocket(ServerWebSocket),
    Sender(oneshot::Sender<ServerWebSocket>),
}

#[derive(Clone, Default)]
pub(crate) struct PendingWebsockets {
    // TODO if websockets are not collected from this map, they pile up and never get removed
    pending_websockets: Arc<Mutex<HashMap<(Uuid, u8), MaybeWebSocket>>>,
}

impl PendingWebsockets {
    pub(crate) async fn get(
        &self,
        request_id: Uuid,
        stream_id: u8,
    ) -> eyre::Result<ServerWebSocket> {
        let mut pending_websockets = self.pending_websockets.lock().await;
        let maybe_websocket = pending_websockets.remove(&(request_id, stream_id));
        match maybe_websocket {
            Some(MaybeWebSocket::WebSocket(websocket)) => Ok(websocket),
            Some(MaybeWebSocket::Sender(_)) => {
                eyre::bail!("tried to get same pending websocket twice")
            }
            None => {
                let (tx, rx) = oneshot::channel();
                pending_websockets.insert((request_id, stream_id), MaybeWebSocket::Sender(tx));
                drop(pending_websockets); // drop to release lock
                Ok(rx.await?)
            }
        }
    }

    pub(crate) async fn insert(
        &self,
        request_id: Uuid,
        stream_id: u8,
        websocket: WebSocket,
    ) -> eyre::Result<()> {
        let (sender, receiver) = websocket.split();
        let websocket = ServerWebSocket { sender, receiver };
        let mut pending_websockets = self.pending_websockets.lock().await;
        let maybe_websocket = pending_websockets.remove(&(request_id, stream_id));
        match maybe_websocket {
            Some(MaybeWebSocket::WebSocket(_)) => {
                eyre::bail!("tried to insert same pending websocket twice")
            }
            Some(MaybeWebSocket::Sender(tx)) => {
                let _ = tx.send(websocket);
            }
            None => {
                pending_websockets.insert(
                    (request_id, stream_id),
                    MaybeWebSocket::WebSocket(websocket),
                );
            }
        }
        Ok(())
    }
}

pub(crate) enum MaybeTcpStream {
    TcpStream(TcpStream),
    Sender(oneshot::Sender<TcpStream>),
}

#[derive(Clone, Default)]
pub(crate) struct PendingTcpStreams {
    // TODO if websockets are not collected from this map, they pile up and never get removed
    pending_tcp_streams: Arc<Mutex<HashMap<(Uuid, u8), MaybeTcpStream>>>,
}

impl PendingTcpStreams {
    pub(crate) async fn new(bind_addr: SocketAddr) -> eyre::Result<Self> {
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        let pending_tcp_streams = Self::default();
        let pending_tcp_streams_clone = pending_tcp_streams.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await?;
                stream.set_nodelay(true)?;
                let mut request_id = [0; 16];
                stream.read_exact(&mut request_id).await?;
                let request_id = Uuid::from_bytes(request_id);
                let stream_id = stream.read_u8().await?;
                pending_tcp_streams_clone
                    .insert(request_id, stream_id, stream)
                    .await?;
            }
            #[allow(unreachable_code)]
            eyre::Ok(())
        });
        Ok(pending_tcp_streams)
    }

    pub(crate) async fn get(&self, request_id: Uuid, stream_id: u8) -> eyre::Result<TcpStream> {
        let mut pending_tcp_streams = self.pending_tcp_streams.lock().await;
        let maybe_tcp_stream = pending_tcp_streams.remove(&(request_id, stream_id));
        match maybe_tcp_stream {
            Some(MaybeTcpStream::TcpStream(stream)) => Ok(stream),
            Some(MaybeTcpStream::Sender(_)) => {
                eyre::bail!("tried to get same pending stream twice")
            }
            None => {
                let (tx, rx) = oneshot::channel();
                pending_tcp_streams.insert((request_id, stream_id), MaybeTcpStream::Sender(tx));
                drop(pending_tcp_streams); // drop to release lock
                Ok(rx.await?)
            }
        }
    }

    pub(crate) async fn insert(
        &self,
        request_id: Uuid,
        stream_id: u8,
        stream: TcpStream,
    ) -> eyre::Result<()> {
        let mut pending_tcp_streams = self.pending_tcp_streams.lock().await;
        let maybe_stream = pending_tcp_streams.remove(&(request_id, stream_id));
        match maybe_stream {
            Some(MaybeTcpStream::TcpStream(_)) => {
                eyre::bail!("tried to insert same pending stream twice")
            }
            Some(MaybeTcpStream::Sender(tx)) => {
                let _ = tx.send(stream);
            }
            None => {
                pending_tcp_streams
                    .insert((request_id, stream_id), MaybeTcpStream::TcpStream(stream));
            }
        }
        Ok(())
    }
}

/// Main application state for the bitservice-server used for Axum.
///
/// If Axum should be able to extract services, it should be added to
/// the `AppState`.
#[derive(Clone)]
pub(crate) struct AppState {
    party_id: PartyID,
    pending_websockets: PendingWebsockets,
    pending_tcp_streams: PendingTcpStreams,
    next_peer: String,
    tcp_mpc_net_next_peer: SocketAddr,
    prev_peer_wait_timeout: Duration,
}

pub async fn start(config: BitservicePeerConfig) -> eyre::Result<()> {
    tracing::info!("starting bitservice-peer with config: {config:#?}");
    if rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .is_err()
    {
        tracing::warn!("cannot install rustls crypto provider!");
        tracing::warn!("we continue but this should not happen...");
    };

    let app_state = AppState {
        party_id: config.party_id.try_into()?,
        pending_websockets: PendingWebsockets::default(),
        pending_tcp_streams: PendingTcpStreams::new(config.tcp_mpc_net_bind_addr).await?,
        next_peer: config.next_peer,
        tcp_mpc_net_next_peer: config.tcp_mpc_net_next_peer,
        prev_peer_wait_timeout: config.prev_peer_wait_timeout,
    };
    let app = api::new_app(app_state);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await?
        .tap_io(|tcp_stream| {
            if let Err(err) = tcp_stream.set_nodelay(true) {
                tracing::warn!("failed to set TCP_NODELAY on incoming connection: {err:#}");
            }
        });
    tracing::info!("starting axum server on {}", config.bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}
