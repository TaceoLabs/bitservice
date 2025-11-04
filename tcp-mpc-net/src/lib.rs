use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, sync::atomic::AtomicUsize};

use eyre::ContextCompat as _;
use futures::{SinkExt as _, StreamExt as _};
use mpc_core::protocols::rep3::id::PartyID;
use mpc_net::{ConnectionStats, Network};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;
use tokio::{net::TcpStream, sync::oneshot};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub enum TcpSession {
    Ready(TcpStream),
    Waiter(oneshot::Sender<TcpStream>),
}

#[derive(Clone)]
pub struct TcpSessions {
    // TODO if streams are not collected from this map, they pile up and never get removed
    sessions: Arc<tokio::sync::Mutex<HashMap<Uuid, TcpSession>>>,
}

impl TcpSessions {
    pub async fn new(bind_addr: SocketAddr) -> eyre::Result<Self> {
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        let sessions = Self {
            sessions: Arc::default(),
        };
        let sessions_clone = sessions.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await?;
                stream.set_nodelay(true)?;
                let mut session_id = [0; 16];
                stream.read_exact(&mut session_id).await?;
                let session_id = Uuid::from_bytes(session_id);
                sessions_clone.insert(session_id, stream).await?;
            }
            #[allow(unreachable_code)]
            eyre::Ok(())
        });
        Ok(sessions)
    }

    pub async fn get(&self, session_id: Uuid) -> eyre::Result<TcpStream> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.remove(&session_id);
        match session {
            Some(TcpSession::Ready(stream)) => Ok(stream),
            Some(TcpSession::Waiter(_)) => {
                eyre::bail!("tried to get same session twice")
            }
            None => {
                let (tx, rx) = oneshot::channel();
                sessions.insert(session_id, TcpSession::Waiter(tx));
                drop(sessions); // drop to release lock
                Ok(rx.await?)
            }
        }
    }

    async fn insert(&self, session_id: Uuid, stream: TcpStream) -> eyre::Result<()> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions.remove(&session_id);
        match session {
            Some(TcpSession::Ready(_)) => {
                eyre::bail!("tried to insert same session twice")
            }
            Some(TcpSession::Waiter(tx)) => {
                let _ = tx.send(stream);
            }
            None => {
                sessions.insert(session_id, TcpSession::Ready(stream));
            }
        }
        Ok(())
    }
}

pub async fn tcp_connect(addr: SocketAddr, session_id: Uuid) -> eyre::Result<TcpStream> {
    tracing::debug!("connecting to peer: {addr}");

    let mut stream = TcpStream::connect(addr).await?;

    stream.set_nodelay(true)?;
    stream.write_all(&session_id.into_bytes()).await?;

    tracing::debug!("connected");
    Ok(stream)
}

#[derive(Debug)]
#[expect(clippy::complexity)]
pub struct TcpNetwork {
    id: PartyID,
    // TODO could replace map with something simpler, we only need 3 parties
    send: HashMap<usize, (mpsc::Sender<Vec<u8>>, AtomicUsize)>,
    recv: HashMap<usize, (Mutex<mpsc::Receiver<eyre::Result<Vec<u8>>>>, AtomicUsize)>,
}

impl TcpNetwork {
    pub fn new(
        id: PartyID,
        next_stream: TcpStream,
        prev_stream: TcpStream,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<Self> {
        let mut send = HashMap::new();
        let mut recv = HashMap::new();

        let codec = LengthDelimitedCodec::new();
        let next_stream = Framed::new(next_stream, codec.clone());
        let prev_stream = Framed::new(prev_stream, codec);

        let (mut next_sender, mut next_receiver) = next_stream.split();
        let (mut prev_sender, mut prev_receiver) = prev_stream.split();

        // TODO deduplicate for prev and next
        let (next_send_tx, mut next_send_rx) = mpsc::channel::<Vec<u8>>(32);
        let (next_recv_tx, next_recv_rx) = mpsc::channel::<eyre::Result<Vec<u8>>>(32);
        tokio::task::spawn(async move {
            while let Some(data) = next_send_rx.recv().await {
                if let Err(err) = next_sender.send(data.into()).await {
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
                            Some(Ok(data)) => {
                                if next_recv_tx.send(Ok(data.into())).await.is_err() {
                                    tracing::warn!("recv receiver dropped");
                                    break;
                                }
                            }
                            Some(Err(err)) => {
                                let _ = next_recv_tx.send(Err(eyre::eyre!("tcp error: {err}"))).await;
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
                if let Err(err) = prev_sender.send(data.into()).await {
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
                            Some(Ok(data)) => {
                                if prev_recv_tx.send(Ok(data.into())).await.is_err() {
                                    tracing::warn!("recv receiver dropped");
                                    break;
                                }
                            }
                            Some(Err(err)) => {
                                let _ = prev_recv_tx.send(Err(eyre::eyre!("tcp error: {err}"))).await;
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

impl Network for TcpNetwork {
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
