use std::{collections::HashMap, net::SocketAddr, path::PathBuf, time::Duration};

use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};

/// The environment the service is running in.
///
/// Main usage for the `Environment` is to call
/// [`Environment::assert_is_dev`]. Services that are intended
/// for `dev` only (like SC mock watcher, local secret-manager,...)
/// shall assert that they are called from the `dev` environment.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Environment {
    /// Production environment.
    Prod,
    /// Development environment.
    Dev,
}

impl Environment {
    /// Asserts that `Environment` is `dev`. Panics if not the case.
    pub fn assert_is_dev(&self) {
        assert!(matches!(self, Environment::Dev), "Is not dev environment")
    }
}

/// The configuration for the bitservice server.
///
/// It can be configured via environment variables or command line arguments using `clap`.
#[derive(Parser, Debug)]
pub struct BitserviceServerConfig {
    /// The environment of bitservice (either `prod` or `dev`).
    #[clap(long, env = "BITSERVICE_ENVIRONMENT", default_value = "prod")]
    pub environment: Environment,

    /// The bind addr of the AXUM server
    #[clap(long, env = "BITSERVICE_BIND_ADDR", default_value = "0.0.0.0:4321")]
    pub bind_addr: SocketAddr,

    /// Path to the RP id -> bitservice peer map config file
    #[clap(long, env = "BITSERVICE_RP_BITSERVICE_PEERS_CONFIG_PATH")]
    pub rp_bitservice_peers_config: PathBuf,

    /// The timeout for a request to a peer
    #[clap(
        long,
        env = "BITSERVICE_PEER_REQUEST_TIMEOUT",
        default_value="60s",
        value_parser = humantime::parse_duration
    )]
    pub peer_request_timeout: Duration,

    /// The amount of write after which a prune request should be sent
    #[clap(long, env = "BITSERVICE_PRUNE_WRITE_INTERVAL", default_value = "128")]
    pub prune_write_interval: usize,

    /// The max amount of read tasks who are not yet joined
    ///
    /// This limit only exists to limit the amount of JoinHandles in memory if we encounter many reads without a write
    #[clap(long, env = "BITSERVICE_MAX_NUM_READ_TASKS", default_value = "4096")]
    pub max_num_read_tasks: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RpBitservicePeersConfig {
    pub rp_bitservice_peers: HashMap<String, [String; 3]>,
}
