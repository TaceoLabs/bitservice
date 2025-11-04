use std::collections::HashMap;

use axum::serve::ListenerExt;

use crate::{
    config::{BitserviceServerConfig, RpBitservicePeersConfig},
    rw_queue::RpRwQueue,
};

pub(crate) mod api;
pub mod config;
pub mod metrics;
pub(crate) mod rw_queue;

#[derive(Clone)]
pub(crate) struct RpBitService {
    pub(crate) rw_queue: RpRwQueue,
}

/// Main application state for the bitservice-server used for Axum.
///
/// If Axum should be able to extract services, it should be added to
/// the `AppState`.
#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) rp_bitservices: HashMap<u128, RpBitService>,
}

pub async fn start(config: BitserviceServerConfig) -> eyre::Result<()> {
    tracing::info!("starting bitservice-server with config: {config:#?}");

    let rp_bitservice_peers_config = toml::from_slice::<RpBitservicePeersConfig>(&std::fs::read(
        config.rp_bitservice_peers_config,
    )?)?;

    let rp_bitservice_peers = rp_bitservice_peers_config
        .rp_bitservice_peers
        .into_iter()
        .map(|(rp_id, bitservice_peers)| Ok((rp_id.parse()?, bitservice_peers)))
        .collect::<eyre::Result<HashMap<u128, [String; 3]>>>()?;
    let rp_bitservices = rp_bitservice_peers
        .into_iter()
        .map(|(rp_id, peers)| {
            (
                rp_id,
                RpBitService {
                    rw_queue: RpRwQueue::new(
                        peers,
                        config.prune_write_interval,
                        config.max_num_read_tasks,
                        config.peer_request_timeout,
                    ),
                },
            )
        })
        .collect();

    let app_state = AppState { rp_bitservices };
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
