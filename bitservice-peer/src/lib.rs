use std::sync::Arc;

use axum::serve::ListenerExt as _;
use co_noir_to_r1cs::noir::{r1cs, ultrahonk};
use oblivious_linear_scan_map::Groth16Material;
use rand::{SeedableRng as _, rngs::StdRng};
use secrecy::ExposeSecret;

use crate::{
    ban_service::BanService, config::BitservicePeerConfig, crypto_device::CryptoDevice,
    repository::DbPool,
};

pub(crate) mod api;
pub(crate) mod ban_service;
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
    ban_service: BanService,
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

    let app_state = AppState {
        ban_service: BanService::new(
            config.party_id.try_into()?,
            config.tcp_mpc_net_bind_addr,
            config.next_peer,
            config.tcp_next_peer,
            config.prev_peer_wait_timeout,
            read_groth16,
            write_groth16,
            crypto_device,
            db,
        )
        .await?,
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
