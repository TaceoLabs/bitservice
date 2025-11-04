use std::{fs::File, sync::Arc};

use ark_groth16::ProvingKey;
use ark_serialize::CanonicalDeserialize;
use axum::serve::ListenerExt as _;
use circom_types::groth16::ConstraintMatricesWrapper;
use oblivious_linear_scan_map::Groth16Material;
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

    let proof_schema =
        serde_json::from_reader(File::open(&config.oblivious_map_read_proof_schema_path)?)?;
    let matrices = ConstraintMatricesWrapper::deserialize_compressed(File::open(
        &config.oblivious_map_read_matrices_path,
    )?)?
    .0;
    let pk = ProvingKey::deserialize_compressed(File::open(&config.oblivious_map_read_pk_path)?)?;
    let read_groth16 = Groth16Material::new(proof_schema, matrices, pk);

    let proof_schema =
        serde_json::from_reader(File::open(&config.oblivious_map_write_proof_schema_path)?)?;
    let matrices = ConstraintMatricesWrapper::deserialize_compressed(File::open(
        &config.oblivious_map_write_matrices_path,
    )?)?
    .0;
    let pk = ProvingKey::deserialize_compressed(File::open(&config.oblivious_map_write_pk_path)?)?;
    let write_groth16 = Groth16Material::new(proof_schema, matrices, pk);

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
