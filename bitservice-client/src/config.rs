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

    /// The path to the read proving key
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_OBLIVIOUS_MAP_READ_PK_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_read_pk.bin")
    )]
    pub oblivious_map_read_pk_path: PathBuf,

    /// The path to the write proving key
    #[clap(
        long,
        env = "BITSERVICE_CLIENT_OBLIVIOUS_MAP_WRITE_PK_PATH",
        default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../artifacts/oblivious_map_write_pk.bin")
    )]
    pub oblivious_map_write_pk_path: PathBuf,

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
