use std::{sync::Arc, time::Duration};

use bitservice_types::{
    ban::{BanRequest, BanResponse},
    prune::{PeerPruneRequest, PeerPruneResponse},
    read::{ReadRequest, ReadResponse},
    unban::{UnbanRequest, UnbanResponse},
};
use eyre::Context as _;
use reqwest::IntoUrl;
use serde::{Serialize, de::DeserializeOwned};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
};
use uuid::Uuid;

pub(crate) struct ReadMsg {
    pub(crate) req: ReadRequest,
    pub(crate) request_id: Uuid,
    pub(crate) sender: oneshot::Sender<eyre::Result<ReadResponse>>,
}

pub(crate) struct BanMsg {
    pub(crate) req: BanRequest,
    pub(crate) request_id: Uuid,
    pub(crate) sender: oneshot::Sender<eyre::Result<BanResponse>>,
}

pub(crate) struct UnbanMsg {
    pub(crate) req: UnbanRequest,
    pub(crate) request_id: Uuid,
    pub(crate) sender: oneshot::Sender<eyre::Result<UnbanResponse>>,
}

pub(crate) enum WriteMsg {
    Ban(BanMsg),
    Unban(UnbanMsg),
}

pub(crate) enum RpRwQueueMsg {
    Read(Box<ReadMsg>),
    Write(Box<WriteMsg>),
}

#[derive(Clone)]
pub(crate) struct RpRwQueue {
    queue: mpsc::Sender<RpRwQueueMsg>,
}

impl RpRwQueue {
    pub(crate) fn new(
        peers: [String; 3],
        prune_write_interval: usize,
        max_num_read_tasks: usize,
        request_timeout: Duration,
    ) -> Self {
        let (tx, mut rx) = mpsc::channel(32); // TODO or unbounded?
        let mut read_tasks = JoinSet::new();
        let client = reqwest::Client::builder()
            .timeout(request_timeout)
            .build()
            .expect("can build client");
        let mut prune_write_counter = 0;
        let peers = Arc::new(peers);
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let client = client.clone();
                let peers = Arc::clone(&peers);
                match msg {
                    RpRwQueueMsg::Read(read) => {
                        let ReadMsg {
                            req,
                            request_id,
                            sender,
                        } = *read;
                        tracing::debug!("got read request {request_id}");
                        if read_tasks.len() == max_num_read_tasks {
                            tracing::debug!(
                                "read_tasks len reached {max_num_read_tasks} - join_next to free up space for new task"
                            );
                            // at least one task is likely finished, so this should be fast
                            // on the next write, all read tasks are cleared
                            read_tasks.join_next().await;
                        }
                        read_tasks.spawn(async move {
                            match do_peer_read(client, &peers, req, request_id).await {
                                Ok(res) => {
                                    tracing::debug!("read request {request_id} done");
                                    let _ = sender.send(Ok(res));
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        "read requests {request_id} to peers failed: {err:?}"
                                    );
                                    let _ = sender.send(Err(err));
                                }
                            }
                        });
                    }
                    RpRwQueueMsg::Write(write_msg) => {
                        tracing::debug!("got write");
                        let reads = std::mem::take(&mut read_tasks);
                        tracing::debug!("waiting for {} read tasks to be done", reads.len());
                        reads.join_all().await;
                        tracing::debug!("all read tasks are done");
                        match *write_msg {
                            WriteMsg::Ban(BanMsg {
                                req,
                                request_id,
                                sender,
                            }) => {
                                tracing::debug!("got ban request {request_id}");
                                match do_peer_ban(client.clone(), &peers, req, request_id).await {
                                    Ok(res) => {
                                        tracing::debug!("ban request {request_id} done");
                                        let _ = sender.send(Ok(res));
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            "ban requests {request_id} to peers failed: {err:?}"
                                        );
                                        let _ = sender.send(Err(err));
                                    }
                                }
                            }
                            WriteMsg::Unban(UnbanMsg {
                                req,
                                request_id,
                                sender,
                            }) => {
                                tracing::debug!("got unban request {request_id}");
                                match do_peer_unban(client.clone(), &peers, req, request_id).await {
                                    Ok(res) => {
                                        tracing::debug!("unban request {request_id} done");
                                        let _ = sender.send(Ok(res));
                                    }
                                    Err(err) => {
                                        tracing::warn!(
                                            "unban requests {request_id} to peers failed: {err:?}"
                                        );
                                        let _ = sender.send(Err(err));
                                    }
                                }
                            }
                        }
                        // check if we need to send a prune request after reaching a set number or writes
                        prune_write_counter += 1;
                        if prune_write_counter == prune_write_interval {
                            tracing::debug!(
                                "reached prune_write_interval {prune_write_interval} - send prune request"
                            );
                            let request_id = Uuid::new_v4();
                            match do_peer_prune(client, &peers, request_id).await {
                                Ok(_) => {
                                    tracing::debug!("prune request {request_id} done");
                                }
                                Err(err) => {
                                    tracing::warn!(
                                        "prune requests {request_id} to peers failed: {err:?}"
                                    );
                                }
                            }
                            prune_write_counter = 0;
                        }
                    }
                }
            }
        });
        Self { queue: tx }
    }

    pub(crate) async fn read(&self, req: ReadRequest) -> eyre::Result<ReadResponse> {
        let request_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.queue
            .send(RpRwQueueMsg::Read(
                ReadMsg {
                    req,
                    request_id,
                    sender: tx,
                }
                .into(),
            ))
            .await?;
        rx.await?
    }

    pub(crate) async fn ban(&self, req: BanRequest) -> eyre::Result<BanResponse> {
        let request_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.queue
            .send(RpRwQueueMsg::Write(
                WriteMsg::Ban(BanMsg {
                    req,
                    request_id,
                    sender: tx,
                })
                .into(),
            ))
            .await?;
        rx.await?
    }

    pub(crate) async fn unban(&self, req: UnbanRequest) -> eyre::Result<UnbanResponse> {
        let request_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.queue
            .send(RpRwQueueMsg::Write(
                WriteMsg::Unban(UnbanMsg {
                    req,
                    request_id,
                    sender: tx,
                })
                .into(),
            ))
            .await?;
        rx.await?
    }
}

async fn do_peer_read(
    client: reqwest::Client,
    peers: &[String; 3],
    req: ReadRequest,
    request_id: Uuid,
) -> eyre::Result<ReadResponse> {
    tracing::debug!("send read request {request_id} to peers {peers:?}");
    let urls = peers
        .clone()
        .map(|peer| format!("{peer}/api/v1/read/{request_id}"));
    let responses = post_to_peers(client, urls, &req.requests).await?;
    tracing::debug!("got read response for request {request_id}");
    Ok(ReadResponse { responses })
}

async fn do_peer_ban(
    client: reqwest::Client,
    peers: &[String; 3],
    req: BanRequest,
    request_id: Uuid,
) -> eyre::Result<BanResponse> {
    tracing::debug!("send ban request {request_id} to peers {peers:?}");
    let urls = peers
        .clone()
        .map(|peer| format!("{peer}/api/v1/ban/{request_id}"));
    let responses = post_to_peers(client, urls, &req.requests).await?;
    tracing::debug!("got ban response for request {request_id}");
    Ok(BanResponse { responses })
}

async fn do_peer_unban(
    client: reqwest::Client,
    peers: &[String; 3],
    req: UnbanRequest,
    request_id: Uuid,
) -> eyre::Result<UnbanResponse> {
    tracing::debug!("send unban request {request_id} to peers {peers:?}");
    let urls = peers
        .clone()
        .map(|peer| format!("{peer}/api/v1/unban/{request_id}"));
    let responses = post_to_peers(client, urls, &req.requests).await?;
    tracing::debug!("got unban response for request {request_id}");
    Ok(UnbanResponse { responses })
}

async fn do_peer_prune(
    client: reqwest::Client,
    peers: &[String; 3],
    request_id: Uuid,
) -> eyre::Result<()> {
    tracing::debug!("send prune request {request_id} to peers {peers:?}");
    let urls = peers
        .clone()
        .map(|peer| format!("{peer}/api/v1/prune/{request_id}"));
    let req = PeerPruneRequest {};
    let requests = [req, req, req];
    let _ = post_to_peers::<_, _, PeerPruneResponse>(client, urls, &requests).await?;
    tracing::debug!("got prune response for request {request_id}");
    Ok(())
}

async fn post_to_peers<U: IntoUrl, Req: Serialize, Res: DeserializeOwned>(
    client: reqwest::Client,
    [url0, url1, url2]: [U; 3],
    requests: &[Req; 3],
) -> eyre::Result<[Res; 3]> {
    let (res0, res1, res2) = tokio::join!(
        client.post(url0).json(&requests[0]).send(),
        client.post(url1).json(&requests[1]).send(),
        client.post(url2).json(&requests[2]).send(),
    );
    let res0 = res0.context("while sending request to peer0")?;
    let res1 = res1.context("while sending request to peer1")?;
    let res2 = res2.context("while sending request to peer2")?;
    if !res0.status().is_success() {
        let error = res0.text().await?;
        eyre::bail!("peer0 return error: {error}");
    }
    if !res1.status().is_success() {
        let error = res1.text().await?;
        eyre::bail!("peer1 return error: {error}");
    }
    if !res2.status().is_success() {
        let error = res2.text().await?;
        eyre::bail!("peer2 return error: {error}");
    }
    let (res0, res1, res2) =
        tokio::join!(res0.json::<Res>(), res1.json::<Res>(), res2.json::<Res>(),);
    let res0 = res0.context("while receiving response from peer0")?;
    let res1 = res1.context("while receiving response from peer1")?;
    let res2 = res2.context("while receiving response from peer2")?;
    Ok([res0, res1, res2])
}
