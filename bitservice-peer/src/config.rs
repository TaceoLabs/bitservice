use std::{net::SocketAddr, path::PathBuf, time::Duration};

use clap::{Parser, ValueEnum};
use secrecy::SecretString;

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

/// The configuration for the bitservice peer.
///
/// It can be configured via environment variables or command line arguments using `clap`.
#[derive(Parser, Debug)]
pub struct BitservicePeerConfig {
    /// The environment of bitservice (either `prod` or `dev`).
    #[clap(long, env = "BITSERVICE_PEER_ENVIRONMENT", default_value = "prod")]
    pub environment: Environment,

    /// The bind addr of the AXUM server
    #[clap(long, env = "BITSERVICE_PEER_BIND_ADDR")]
    pub bind_addr: SocketAddr,

    /// The bind addr of the tcp mpc-net server
    #[clap(long, env = "BITSERVICE_PEER_BIND_ADDR")]
    pub tcp_mpc_net_bind_addr: SocketAddr,

    /// The party id of the peer
    #[clap(long, env = "BITSERVICE_PEER_PARTY_ID", value_parser = clap::value_parser!(u8).range(..3))]
    pub party_id: u8,

    /// The ws url of the next peer
    #[clap(long, env = "BITSERVICE_PEER_NEXT_PEER")]
    pub next_peer: String,

    /// The socket addr of the next peer
    #[clap(long, env = "BITSERVICE_PEER_TCP_NEXT_PEER")]
    pub tcp_next_peer: SocketAddr,

    /// The timeout for waiting for a connection from the prev peer
    #[clap(
        long,
        env = "BITSERVICE_PREV_PEER_WAIT_TIMEOUT",
        default_value="30s",
        value_parser = humantime::parse_duration
    )]
    pub prev_peer_wait_timeout: Duration,

    /// The path to the read proving key
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_READ_PK_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_read_pk.bin")
    )]
    pub oblivious_map_read_pk_path: PathBuf,

    /// The path to the read constraint matrices
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_READ_MATRICES_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_read_matrices.bin")
    )]
    pub oblivious_map_read_matrices_path: PathBuf,

    /// The path to the read proof schema
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_READ_PROOF_SCHEMA_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_read_proof_schema.json")
    )]
    pub oblivious_map_read_proof_schema_path: PathBuf,

    /// The path to the write proving key
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_WRITE_PK_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_write_pk.bin")
    )]
    pub oblivious_map_write_pk_path: PathBuf,

    /// The path to the write constraint matrices
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_WRITE_MATRICES_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_write_matrices.bin")
    )]
    pub oblivious_map_write_matrices_path: PathBuf,

    /// The path to the write proof schema
    #[clap(
        long,
        env = "BITSERVICE_PEER_OBLIVIOUS_MAP_WRITE_PROOF_SCHEMA_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_write_proof_schema.json")
    )]
    pub oblivious_map_write_proof_schema_path: PathBuf,

    // TODO probably move to AWS secrets manager
    /// The path to the peer secret key
    #[clap(long, env = "BITSERVICE_PEER_SECRET_KEY_PATH")]
    pub secret_key_path: PathBuf,

    /// The URL for the peer's DB
    #[clap(long, env = "BITSERVICE_PEER_DB_URL")]
    pub db_url: SecretString,
}
