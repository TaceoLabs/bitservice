use std::process::ExitCode;

use ark_ff::UniformRand as _;
use bitservice_client::config::{BitserviceClientCommand, BitserviceClientConfig};
use clap::Parser;
use co_noir_to_r1cs::noir::{r1cs, ultrahonk};
use rand::{SeedableRng as _, rngs::StdRng};

#[tokio::main]
async fn main() -> eyre::Result<ExitCode> {
    nodes_telemetry::install_tracing("bitservice_client=info");
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("can install");

    let config = BitserviceClientConfig::parse();

    let read_program = ultrahonk::get_program_artifact(&config.oblivious_map_read_circuit_path)?;
    let (_, read_pk, _) = r1cs::setup_r1cs(read_program, &mut StdRng::from_seed([0; 32]))?;

    let write_program = ultrahonk::get_program_artifact(&config.oblivious_map_write_circuit_path)?;
    let (_, write_pk, _) = r1cs::setup_r1cs(write_program, &mut StdRng::from_seed([0; 32]))?;

    if config.public_key_paths.len() != 3 {
        eyre::bail!("must provide exactly 3 public key paths");
    }
    let peer_public_keys = config
        .public_key_paths
        .into_iter()
        .map(|path| {
            let bytes = std::fs::read(path)?;
            let public_key = crypto_box::PublicKey::from_slice(&bytes)?;
            Ok(public_key)
        })
        .collect::<eyre::Result<Vec<_>>>()?
        .try_into()
        .expect("len is 3");

    let client = bitservice_client::Client::new(
        reqwest::Client::new(),
        config.server_url,
        config.rp_id,
        peer_public_keys,
        read_pk.vk,
        write_pk.vk,
    );

    let mut rng = rand::thread_rng();

    match config.command {
        BitserviceClientCommand::Read => {
            let value = client
                .read(config.key, ark_bn254::Fr::rand(&mut rng), &mut rng)
                .await?;
            tracing::info!("value = {value}");
        }
        BitserviceClientCommand::Ban => {
            client
                .ban(
                    config.key,
                    ark_bn254::Fr::rand(&mut rng),
                    ark_bn254::Fr::rand(&mut rng),
                    &mut rng,
                )
                .await?;
        }
        BitserviceClientCommand::Unban => {
            client
                .unban(
                    config.key,
                    ark_bn254::Fr::rand(&mut rng),
                    ark_bn254::Fr::rand(&mut rng),
                    &mut rng,
                )
                .await?;
        }
    }

    Ok(ExitCode::SUCCESS)
}
