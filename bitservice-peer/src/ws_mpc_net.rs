use std::{collections::HashMap, sync::atomic::AtomicUsize, time::Duration};

use eyre::ContextCompat as _;
use futures::{SinkExt as _, StreamExt as _};
use mpc_core::protocols::rep3::id::PartyID;
use mpc_net::{ConnectionStats, Network};
use parking_lot::Mutex;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tokio_util::sync::CancellationToken;

use crate::ServerWebSocket;

#[derive(Debug)]
#[expect(clippy::complexity)]
pub struct WebSocketNetwork {
    id: PartyID,
    // TODO replace map with something simpler, we only need 3 parties
    send: HashMap<usize, (mpsc::Sender<Vec<u8>>, AtomicUsize)>,
    recv: HashMap<usize, (Mutex<mpsc::Receiver<eyre::Result<Vec<u8>>>>, AtomicUsize)>,
    _timeout: Duration,
}

impl WebSocketNetwork {
    pub fn new(
        id: PartyID,
        next_websocket: WebSocketStream<MaybeTlsStream<TcpStream>>,
        prev_websocket: ServerWebSocket,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<Self> {
        let mut send = HashMap::new();
        let mut recv = HashMap::new();

        let (mut next_sender, mut next_receiver) = next_websocket.split();
        let (mut prev_sender, mut prev_receiver) = (prev_websocket.sender, prev_websocket.receiver);

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

        Ok(Self {
            id,
            send,
            recv,
            _timeout: Duration::from_secs(10),
        })
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
