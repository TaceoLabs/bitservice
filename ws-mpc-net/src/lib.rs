use std::str::FromStr as _;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, sync::atomic::AtomicUsize};

use axum::extract::WebSocketUpgrade;
use axum::extract::ws::WebSocket;
use axum::response::Response;
use eyre::{Context as _, ContextCompat as _};
use futures::{SinkExt as _, StreamExt as _};
use http::{HeaderMap, HeaderValue, StatusCode};
use mpc_core::protocols::rep3::id::PartyID;
use mpc_net::{ConnectionStats, Network};
use tokio::sync::mpsc;
use tokio::{net::TcpStream, sync::oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest as _;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const SESSION_ID_HEADER: &str = "session_id";

pub type ServerWsStream = WebSocket;

pub type ClientWsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[expect(clippy::large_enum_variant)]
pub enum WsSession {
    Ready(ServerWsStream),
    Waiter(oneshot::Sender<ServerWsStream>),
}

#[derive(Clone, Default)]
pub struct WsSessions {
    // TODO if websockets are not collected from this map, they pile up and never get removed
    sessions: Arc<tokio::sync::Mutex<HashMap<Uuid, WsSession>>>,
}

impl WsSessions {
    pub async fn get(&self, session_id: Uuid) -> eyre::Result<WebSocket> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.remove(&session_id);
        match session {
            Some(WsSession::Ready(websocket)) => Ok(websocket),
            Some(WsSession::Waiter(_)) => {
                eyre::bail!("tried to get same session twice")
            }
            None => {
                let (tx, rx) = oneshot::channel();
                sessions.insert(session_id, WsSession::Waiter(tx));
                drop(sessions); // drop to release lock
                Ok(rx.await?)
            }
        }
    }

    async fn insert(&self, session_id: Uuid, websocket: WebSocket) -> eyre::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.remove(&session_id);
        match session {
            Some(WsSession::Ready(_)) => {
                eyre::bail!("tried to insert same session twice")
            }
            Some(WsSession::Waiter(tx)) => {
                let _ = tx.send(websocket);
            }
            None => {
                sessions.insert(session_id, WsSession::Ready(websocket));
            }
        }
        Ok(())
    }

    pub async fn handle_ws_request(
        &self,
        headers: HeaderMap,
        ws: WebSocketUpgrade,
    ) -> axum::response::Result<Response> {
        let session_id = Uuid::from_str(
            headers
                .get(SESSION_ID_HEADER)
                .ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("missing header \"{SESSION_ID_HEADER}\""),
                    )
                })?
                .to_str()
                .map_err(|_| {
                    (
                        StatusCode::BAD_REQUEST,
                        format!("invalid header value for \"{SESSION_ID_HEADER}\""),
                    )
                })?,
        )
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid header value for \"{SESSION_ID_HEADER}\""),
            )
        })?;
        tracing::debug!("ws upgrade for session {session_id}");
        let sessions = self.clone();
        let response = ws.on_upgrade(move |socket| async move {
            if let Err(err) = sessions.insert(session_id, socket).await {
                tracing::warn!("failed to insert pending websocket: {err:?}");
            }
        });
        Ok(response)
    }
}

pub async fn ws_connect(ws_url: &str, session_id: Uuid) -> eyre::Result<ClientWsStream> {
    let mut request = ws_url
        .into_client_request()
        .context("while creating ws request")?;
    request.headers_mut().insert(
        SESSION_ID_HEADER,
        HeaderValue::from_str(&session_id.to_string()).expect("can convert Uuid to HeaderValue"),
    );
    tracing::debug!("connecting to peer: {ws_url}");

    let (websocket, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("while connecting to websocket")?;
    tracing::debug!("connected");

    websocket.get_ref().get_ref().set_nodelay(true)?;
    Ok(websocket)
}

#[derive(Debug)]
#[expect(clippy::complexity)]
pub struct WebSocketNetwork {
    id: PartyID,
    // TODO could replace map with something simpler, we only need 3 parties
    send: HashMap<usize, (mpsc::Sender<Vec<u8>>, AtomicUsize)>,
    recv: HashMap<usize, (Mutex<mpsc::Receiver<eyre::Result<Vec<u8>>>>, AtomicUsize)>,
}

impl WebSocketNetwork {
    pub fn new(
        id: PartyID,
        next_websocket: ClientWsStream,
        prev_websocket: ServerWsStream,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<Self> {
        let mut send = HashMap::new();
        let mut recv = HashMap::new();

        let (mut next_sender, mut next_receiver) = next_websocket.split();
        let (mut prev_sender, mut prev_receiver) = prev_websocket.split();

        // TODO deduplicate for prev and next
        let (next_send_tx, mut next_send_rx) = mpsc::channel::<Vec<u8>>(32);
        let (next_recv_tx, next_recv_rx) = mpsc::channel::<eyre::Result<Vec<u8>>>(32);
        tokio::task::spawn(async move {
            while let Some(data) = next_send_rx.recv().await {
                if let Err(err) = next_sender
                    .send(tokio_tungstenite::tungstenite::Message::Binary(data.into()))
                    .await
                {
                    tracing::warn!("failed to send data: {err:?}");
                    break;
                }
            }
        });
        let cancellation_token_clone = cancellation_token.clone();
        tokio::task::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancellation_token_clone.cancelled() => {
                        break;
                    }
                    msg = next_receiver.next() => {
                        match msg {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                                if next_recv_tx.send(Ok(data.into())).await.is_err() {
                                    tracing::warn!("recv receiver dropped");
                                    break;
                                }
                            }
                            Some(Ok(_)) => {
                                tracing::warn!("unexpected ws message: {msg:?}");
                                let _ = next_recv_tx.send(Err(eyre::eyre!("invalid ws message"))).await;
                                break;
                            }
                            Some(Err(err)) => {
                                let _ = next_recv_tx.send(Err(eyre::eyre!("websocket error: {err}"))).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        let (prev_send_tx, mut prev_send_rx) = mpsc::channel::<Vec<u8>>(32);
        let (prev_recv_tx, prev_recv_rx) = mpsc::channel::<eyre::Result<Vec<u8>>>(32);
        tokio::task::spawn(async move {
            while let Some(data) = prev_send_rx.recv().await {
                if let Err(err) = prev_sender
                    .send(axum::extract::ws::Message::Binary(data.into()))
                    .await
                {
                    tracing::warn!("failed to send data: {err:?}");
                    break;
                }
            }
        });
        let cancellation_token_clone = cancellation_token.clone();
        tokio::task::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancellation_token_clone.cancelled() => {
                        break;
                    }
                    msg = prev_receiver.next() => {
                        match msg {
                            Some(Ok(axum::extract::ws::Message::Binary(data))) => {
                                if prev_recv_tx.send(Ok(data.into())).await.is_err() {
                                    tracing::warn!("recv receiver dropped");
                                    break;
                                }
                            }
                            Some(Ok(_)) => {
                                tracing::warn!("unexpected ws message: {msg:?}");
                                let _ = prev_recv_tx.send(Err(eyre::eyre!("invalid ws message"))).await;
                                break;
                            }
                            Some(Err(err)) => {
                                let _ = prev_recv_tx.send(Err(eyre::eyre!("websocket error: {err}"))).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        send.insert(id.next().into(), (next_send_tx, AtomicUsize::default()));
        send.insert(id.prev().into(), (prev_send_tx, AtomicUsize::default()));
        recv.insert(
            id.next().into(),
            (Mutex::new(next_recv_rx), AtomicUsize::default()),
        );
        recv.insert(
            id.prev().into(),
            (Mutex::new(prev_recv_rx), AtomicUsize::default()),
        );

        Ok(Self { id, send, recv })
    }
}

impl Network for WebSocketNetwork {
    fn id(&self) -> usize {
        self.id.into()
    }

    fn send(&self, to: usize, data: &[u8]) -> eyre::Result<()> {
        let (sender, sent_bytes) = self.send.get(&to).context("party id out-of-bounds")?;
        sent_bytes.fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
        sender.blocking_send(data.to_vec())?;
        Ok(())
    }

    fn recv(&self, from: usize) -> eyre::Result<Vec<u8>> {
        let (receiver, recv_bytes) = self.recv.get(&from).context("party id out-of-bounds")?;
        let data = receiver
            .lock()
            .expect("not poisoned")
            .blocking_recv()
            .context("receiver sender dropped")??;
        recv_bytes.fetch_add(data.len(), std::sync::atomic::Ordering::Relaxed);
        Ok(data)
    }

    fn get_connection_stats(&self) -> ConnectionStats {
        let mut stats = std::collections::BTreeMap::new();
        for (id, (_, sent_bytes)) in self.send.iter() {
            let recv_bytes = &self.recv.get(id).expect("was in send so must be in recv").1;
            stats.insert(
                *id,
                (
                    sent_bytes.load(std::sync::atomic::Ordering::Relaxed),
                    recv_bytes.load(std::sync::atomic::Ordering::Relaxed),
                ),
            );
        }
        ConnectionStats::new(self.id.into(), stats)
    }
}
