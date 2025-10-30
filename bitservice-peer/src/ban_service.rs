use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use mpc_core::protocols::rep3::{Rep3State, conversion::A2BType, id::PartyID};
use oblivious_linear_scan_map::{
    Groth16Material, LinearScanObliviousMap, ObliviousReadRequest, ObliviousReadResult,
    ObliviousUpdateRequest, ObliviousWriteResult,
};
use rand::{Rng as _, SeedableRng as _, rngs::StdRng};
use tcp_mpc_net::{TcpNetwork, TcpSessions};
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::instrument;
use uuid::Uuid;
use ws_mpc_net::{WebSocketNetwork, WsSessions};

use crate::repository::DbPool;

#[derive(Clone)]
pub(crate) struct BanService {
    party_id: PartyID,
    pub(crate) ws_sessions: WsSessions,
    #[allow(dead_code)]
    tcp_sessions: TcpSessions,
    next_peer: String,
    #[allow(dead_code)]
    tcp_next_peer: SocketAddr,
    prev_peer_wait_timeout: Duration,
    oblivious_map: Arc<RwLock<LinearScanObliviousMap>>,
    read_groth16: Arc<Groth16Material>,
    write_groth16: Arc<Groth16Material>,
    db: Arc<DbPool>,
}

impl BanService {
    #[expect(clippy::too_many_arguments)]
    pub(crate) async fn new(
        party_id: PartyID,
        tcp_mpc_net_bind_addr: SocketAddr,
        next_peer: String,
        tcp_next_peer: SocketAddr,
        prev_peer_wait_timeout: Duration,
        read_groth16: Groth16Material,
        write_groth16: Groth16Material,
        db: DbPool,
    ) -> eyre::Result<Self> {
        let oblivious_map = if let Some(oblivious_map) = db.load_map().await? {
            tracing::info!("loaded map from db");
            oblivious_map
        } else {
            tracing::info!("init default map");
            let oblivious_map = LinearScanObliviousMap::default();
            db.store_map(&oblivious_map).await?;
            oblivious_map
        };

        Ok(Self {
            party_id,
            ws_sessions: WsSessions::default(),
            tcp_sessions: TcpSessions::new(tcp_mpc_net_bind_addr).await?,
            next_peer,
            tcp_next_peer,
            prev_peer_wait_timeout,
            oblivious_map: Arc::new(RwLock::new(oblivious_map)),
            read_groth16: Arc::new(read_groth16),
            write_groth16: Arc::new(write_groth16),
            db: Arc::new(db),
        })
    }

    pub(crate) async fn read(
        &self,
        req: ObliviousReadRequest,
        request_id: Uuid,
    ) -> eyre::Result<ObliviousReadResult> {
        let cancellation_token = CancellationToken::new();
        let (net0, net1) = self
            .init_ws_mpc_nets(request_id, cancellation_token.clone())
            .await?;

        let oblivious_map = self.oblivious_map.read().await;

        let start = Instant::now();
        let res = tokio::task::block_in_place(|| {
            let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
            oblivious_map.oblivious_read(req, &net0, &net1, &mut rep3_state, &self.read_groth16)
        })?;
        tracing::debug!("read took {:?}", start.elapsed());

        cancellation_token.cancel();

        Ok(res)
    }

    pub(crate) async fn ban(
        &self,
        req: ObliviousUpdateRequest,
        request_id: Uuid,
    ) -> eyre::Result<ObliviousWriteResult> {
        let cancellation_token = CancellationToken::new();
        let (net0, net1) = self
            .init_ws_mpc_nets(request_id, cancellation_token.clone())
            .await?;

        let mut oblivious_map = self.oblivious_map.write().await;

        let start = Instant::now();
        let res = tokio::task::block_in_place(|| {
            let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
            oblivious_map.oblivious_insert_or_update(
                req,
                &net0,
                &net1,
                &mut rep3_state,
                &self.write_groth16,
            )
        })?;
        tracing::debug!("ban took {:?}", start.elapsed());

        cancellation_token.cancel();

        tracing::debug!("store map in db");
        self.db.store_map(&oblivious_map).await?;

        Ok(res)
    }

    pub(crate) async fn unban(
        &self,
        req: ObliviousUpdateRequest,
        request_id: Uuid,
    ) -> eyre::Result<ObliviousWriteResult> {
        let cancellation_token = CancellationToken::new();
        let (net0, net1) = self
            .init_ws_mpc_nets(request_id, cancellation_token.clone())
            .await?;

        let mut oblivious_map = self.oblivious_map.write().await;

        let start = Instant::now();
        let res = tokio::task::block_in_place(|| {
            let mut rep3_state = Rep3State::new(&net0, A2BType::default())?;
            oblivious_map.oblivious_update(req, &net0, &net1, &mut rep3_state, &self.write_groth16)
        })?;
        tracing::debug!("unban took {:?}", start.elapsed());

        cancellation_token.cancel();

        tracing::debug!("store map in db");
        self.db.store_map(&oblivious_map).await?;

        Ok(res)
    }

    #[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
    #[expect(dead_code)]
    async fn init_tcp_mpc_nets(
        &self,
        request_id: Uuid,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<(TcpNetwork, TcpNetwork)> {
        tracing::debug!("connecting to next_peer: {}", self.tcp_next_peer);

        let mut session_seed = [0u8; 32];
        session_seed[..16].copy_from_slice(&request_id.into_bytes());
        let mut session_rng = StdRng::from_seed(session_seed);
        let session0 = Uuid::from_bytes(session_rng.r#gen());
        let session1 = Uuid::from_bytes(session_rng.r#gen());

        let (next_stream0, next_stream1) = tokio::join!(
            tcp_mpc_net::tcp_connect(self.tcp_next_peer, session0),
            tcp_mpc_net::tcp_connect(self.tcp_next_peer, session1)
        );
        let next_stream0 = next_stream0?;
        let next_stream1 = next_stream1?;

        tracing::debug!("waiting for prev_peer");
        let (prev_stream0, prev_stream1) =
            tokio::time::timeout(self.prev_peer_wait_timeout, async {
                let prev_stream0 = self.tcp_sessions.get(session0).await?;
                let prev_stream1 = self.tcp_sessions.get(session1).await?;
                eyre::Ok((prev_stream0, prev_stream1))
            })
            .await??;

        tracing::debug!("creating mpc networks");
        let net0 = TcpNetwork::new(
            self.party_id,
            next_stream0,
            prev_stream0,
            cancellation_token.clone(),
        )?;
        let net1 = TcpNetwork::new(
            self.party_id,
            next_stream1,
            prev_stream1,
            cancellation_token.clone(),
        )?;
        tracing::debug!("done");
        Ok((net0, net1))
    }

    #[instrument(level = "debug", skip_all, fields(request_id = %request_id))]
    async fn init_ws_mpc_nets(
        &self,
        request_id: Uuid,
        cancellation_token: CancellationToken,
    ) -> eyre::Result<(WebSocketNetwork, WebSocketNetwork)> {
        tracing::debug!("connecting to next_peer: {}", self.next_peer);

        let mut session_seed = [0u8; 32];
        session_seed[..16].copy_from_slice(&request_id.into_bytes());
        let mut session_rng = StdRng::from_seed(session_seed);
        let session0 = Uuid::from_bytes(session_rng.r#gen());
        let session1 = Uuid::from_bytes(session_rng.r#gen());

        let (next_websocket0, next_websocket1) = tokio::join!(
            ws_mpc_net::ws_connect(&self.next_peer, session0),
            ws_mpc_net::ws_connect(&self.next_peer, session1),
        );
        let next_websocket0 = next_websocket0?;
        let next_websocket1 = next_websocket1?;

        tracing::debug!("waiting for prev_peer");
        let (prev_websocket0, prev_websocket1) =
            tokio::time::timeout(self.prev_peer_wait_timeout, async {
                let prev_websocket0 = self.ws_sessions.get(session0).await?;
                let prev_websocket1 = self.ws_sessions.get(session1).await?;
                eyre::Ok((prev_websocket0, prev_websocket1))
            })
            .await??;

        tracing::debug!("creating mpc networks");
        let net0 = WebSocketNetwork::new(
            self.party_id,
            next_websocket0,
            prev_websocket0,
            cancellation_token.clone(),
        )?;
        let net1 = WebSocketNetwork::new(
            self.party_id,
            next_websocket1,
            prev_websocket1,
            cancellation_token.clone(),
        )?;
        Ok((net0, net1))
    }
}
