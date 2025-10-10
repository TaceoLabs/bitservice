use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::serve::ListenerExt as _;
use co_noir_to_r1cs::noir::{r1cs, ultrahonk};
use mpc_core::protocols::rep3::id::PartyID;
use oblivious_linear_scan_map::{Groth16Material, LinearScanObliviousMap};
use rand::{SeedableRng as _, rngs::StdRng};
use secrecy::ExposeSecret;
use tcp_mpc_net::TcpSessions;
use tokio::sync::RwLock;
use ws_mpc_net::WsSessions;

use crate::{config::BitservicePeerConfig, crypto_device::CryptoDevice, repository::DbPool};

pub(crate) mod api;
pub mod config;
pub(crate) mod crypto_device;
pub mod metrics;
pub(crate) mod repository;

/// Main application state for the bitservice-server used for Axum.
///
/// If Axum should be able to extract services, it should be added to
/// the `AppState`.
#[derive(Clone)]
pub(crate) struct AppState {
    party_id: PartyID,
    ws_sessions: WsSessions,
    tcp_sessions: TcpSessions,
    next_peer: String,
    tcp_next_peer: SocketAddr,
    prev_peer_wait_timeout: Duration,
    crypto_device: Arc<CryptoDevice>,
    oblivious_map: Arc<RwLock<LinearScanObliviousMap>>,
    read_groth16: Arc<Groth16Material>,
    write_groth16: Arc<Groth16Material>,
    db: Arc<DbPool>,
}

pub async fn start(config: BitservicePeerConfig) -> eyre::Result<()> {
    tracing::info!("starting bitservice-peer with config: {config:#?}");

    let db = DbPool::open(config.db_url.expose_secret()).await?;

    let crypto_device = Arc::new(CryptoDevice::new(config.secret_key_path)?);

    let read_program = ultrahonk::get_program_artifact(&config.oblivious_map_read_circuit_path)?;
    let (proof_schema, pk, cs) = r1cs::setup_r1cs(read_program, &mut StdRng::from_seed([0; 32]))?;
    let read_groth16 = Groth16Material::new(proof_schema, cs, pk);

    let write_program = ultrahonk::get_program_artifact(&config.oblivious_map_write_circuit_path)?;
    let (proof_schema, pk, cs) = r1cs::setup_r1cs(write_program, &mut StdRng::from_seed([0; 32]))?;
    let write_groth16 = Groth16Material::new(proof_schema, cs, pk);

    let oblivious_map = if let Some(oblivious_map) = db.load_map().await? {
        tracing::info!("loaded map from db");
        oblivious_map
    } else {
        tracing::info!("init default map");
        let oblivious_map = LinearScanObliviousMap::default();
        db.store_map(&oblivious_map).await?;
        oblivious_map
    };

    let app_state = AppState {
        party_id: config.party_id.try_into()?,
        ws_sessions: WsSessions::default(),
        tcp_sessions: TcpSessions::new(config.tcp_mpc_net_bind_addr).await?,
        next_peer: config.next_peer,
        tcp_next_peer: config.tcp_next_peer,
        prev_peer_wait_timeout: config.prev_peer_wait_timeout,
        crypto_device,
        oblivious_map: Arc::new(RwLock::new(oblivious_map)),
        read_groth16: Arc::new(read_groth16),
        write_groth16: Arc::new(write_groth16),
        db: Arc::new(db),
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
