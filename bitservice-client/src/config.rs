use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, ValueEnum)]
pub enum BitserviceClientCommand {
    Read,
    Ban,
    Unban,
}

/// The configuration for the bitservice client.
///
/// It can be configured via environment variables or command line arguments using `clap`.
#[derive(Parser, Debug)]
pub struct BitserviceClientConfig {
    /// The API url of the bitservice server
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_SERVER_API_URL",
        default_value = "http://localhost:4321"
    )]
    pub server_url: String,

    /// The path to the compiled noir read circuit
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_OBLIVIOUS_MAP_READ_CIRCUIT_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../oblivious_map_read.json")
    )]
    pub oblivious_map_read_circuit_path: PathBuf,

    /// The path to the compiled noir write circuit
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_OBLIVIOUS_MAP_WRITE_CIRCUIT_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../oblivious_map_write.json")
    )]
    pub oblivious_map_write_circuit_path: PathBuf,

    /// The RP id
    #[clap(long, env = "BITSERVICE_CLIENT_RP_ID")]
    pub rp_id: u128,

    /// The paths to the 3 public keys for the bitservice peers for the given RP id
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_PUBLIC_KEY_PATHS",
        value_delimiter = ','
    )]
    pub public_key_paths: Vec<PathBuf>,

    /// The user key
    #[clap(long, env = "BITSERVICE_CLIENT_KEY")]
    pub key: u32,

    /// Command
    #[clap(value_enum, env = "BITSERVICE_CLIENT_COMMAND")]
    pub command: BitserviceClientCommand,
}
