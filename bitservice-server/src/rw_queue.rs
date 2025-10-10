use std::{sync::Arc, time::Duration};

use bitservice_types::{
    PeerPruneRequest, PeerReadRequest, PeerReadResponse, PeerWriteRequest, PeerWriteResponse,
    ReadRequest, ReadResponse, WriteRequest, WriteResponse,
};
use eyre::Context as _;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinSet,
};
use uuid::Uuid;

pub(crate) enum RpRwQueueMsg {
    Read(
        ReadRequest,
        Uuid,
        oneshot::Sender<eyre::Result<ReadResponse>>,
    ),
    Write(
        WriteRequest,
        Uuid,
        oneshot::Sender<eyre::Result<WriteResponse>>,
    ),
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
                    RpRwQueueMsg::Read(req, request_id, sender) => {
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
                    RpRwQueueMsg::Write(req, request_id, sender) => {
                        tracing::debug!("got write request {request_id}");
                        let reads = std::mem::take(&mut read_tasks);
                        tracing::debug!("waiting for {} read tasks to be done", reads.len());
                        reads.join_all().await;
                        tracing::debug!("all read tasks are done");
                        match do_peer_write(client.clone(), &peers, req, request_id).await {
                            Ok(res) => {
                                tracing::debug!("write request {request_id} done");
                                let _ = sender.send(Ok(res));
                            }
                            Err(err) => {
                                tracing::warn!(
                                    "write requests {request_id} to peers failed: {err:?}"
                                );
                                let _ = sender.send(Err(err));
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
            .send(RpRwQueueMsg::Read(req, request_id, tx))
            .await?;
        rx.await?
    }

    pub(crate) async fn write(&self, req: WriteRequest) -> eyre::Result<WriteResponse> {
        let request_id = Uuid::new_v4();
        let (tx, rx) = oneshot::channel();
        self.queue
            .send(RpRwQueueMsg::Write(req, request_id, tx))
            .await?;
        rx.await?
    }
}

async fn do_peer_read(
    client: reqwest::Client,
    peers: &[String; 3],
    _req: ReadRequest,
    request_id: Uuid,
) -> eyre::Result<ReadResponse> {
    let [peer0, peer1, peer2] = peers;
    let req = PeerReadRequest { request_id };
    tracing::debug!("send read request {request_id} to peers [{peer0}, {peer1}, {peer2}]");
    let (response0, response1, response2) = tokio::join!(
        client
            .post(format!("{peer0}/api/v1/read"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer1}/api/v1/read"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer2}/api/v1/read"))
            .json(&req)
            .send(),
    );
    let response0 = response0.context("while sending request to peer0")?;
    let response1 = response1.context("while sending request to peer1")?;
    let response2 = response2.context("while sending request to peer2")?;
    let (response0, response1, response2) = tokio::join!(
        response0.json::<PeerReadResponse>(),
        response1.json::<PeerReadResponse>(),
        response2.json::<PeerReadResponse>(),
    );
    let response0 = response0.context("while receiving response from peer0")?;
    let response1 = response1.context("while receiving response from peer1")?;
    let response2 = response2.context("while receiving response from peer2")?;

    tracing::debug!("got read response for request {request_id}");

    Ok(ReadResponse {
        response0,
        response1,
        response2,
    })
}

async fn do_peer_write(
    client: reqwest::Client,
    peers: &[String; 3],
    _req: WriteRequest,
    request_id: Uuid,
) -> eyre::Result<WriteResponse> {
    let [peer0, peer1, peer2] = peers;
    let req = PeerWriteRequest { request_id };
    tracing::debug!("send write request {request_id} to peers [{peer0}, {peer1}, {peer2}]");
    let (response0, response1, response2) = tokio::join!(
        client
            .post(format!("{peer0}/api/v1/write"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer1}/api/v1/write"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer2}/api/v1/write"))
            .json(&req)
            .send(),
    );
    let response0 = response0.context("while sending request to peer0")?;
    let response1 = response1.context("while sending request to peer1")?;
    let response2 = response2.context("while sending request to peer2")?;
    let (response0, response1, response2) = tokio::join!(
        response0.json::<PeerWriteResponse>(),
        response1.json::<PeerWriteResponse>(),
        response2.json::<PeerWriteResponse>(),
    );
    let response0 = response0.context("while receiving response from peer0")?;
    let response1 = response1.context("while receiving response from peer1")?;
    let response2 = response2.context("while receiving response from peer2")?;

    tracing::debug!("got write response for request {request_id}");

    Ok(WriteResponse {
        response0,
        response1,
        response2,
    })
}

async fn do_peer_prune(
    client: reqwest::Client,
    peers: &[String; 3],
    request_id: Uuid,
) -> eyre::Result<()> {
    let [peer0, peer1, peer2] = peers;
    let req = PeerPruneRequest { request_id };
    tracing::debug!("send prune request {request_id} to peers [{peer0}, {peer1}, {peer2}]");
    let (response0, response1, response2) = tokio::join!(
        client
            .post(format!("{peer0}/api/v1/prune"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer1}/api/v1/prune"))
            .json(&req)
            .send(),
        client
            .post(format!("{peer2}/api/v1/prune"))
            .json(&req)
            .send(),
    );
    let response0 = response0.context("while sending request to peer0")?;
    let response1 = response1.context("while sending request to peer1")?;
    let response2 = response2.context("while sending request to peer2")?;

    if !response0.status().is_success() {
        let error = response0.text().await?;
        eyre::bail!("{error:?}");
    }
    if !response1.status().is_success() {
        let error = response1.text().await?;
        eyre::bail!("{error:?}");
    }
    if !response2.status().is_success() {
        let error = response2.text().await?;
        eyre::bail!("{error:?}");
    }

    tracing::debug!("got prune response for request {request_id}");

    Ok(())
}
