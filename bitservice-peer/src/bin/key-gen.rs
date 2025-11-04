use std::{path::PathBuf, process::ExitCode};

use clap::Parser;

#[derive(Parser, Debug)]
pub struct KeyGenConfig {
    /// The path to write the peer key pairs to
    #[clap(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"),"/../dev-keys"))]
    pub out: PathBuf,
}

fn main() -> eyre::Result<ExitCode> {
    let config = KeyGenConfig::parse();
    let mut rng = rand::thread_rng();
    for i in 0..3 {
        let sk = crypto_box::SecretKey::generate(&mut rng);
        let pk = sk.public_key();
        std::fs::write(config.out.join(format!("peer{i}.sk")), sk.to_bytes())?;
        std::fs::write(config.out.join(format!("peer{i}.pk")), pk.to_bytes())?;
    }
    Ok(ExitCode::SUCCESS)
}
