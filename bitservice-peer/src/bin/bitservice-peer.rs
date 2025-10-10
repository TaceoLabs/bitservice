use std::process::ExitCode;

use bitservice_peer::config::BitservicePeerConfig;
use clap::Parser;
use git_version::git_version;

#[tokio::main]
async fn main() -> eyre::Result<ExitCode> {
    let tracing_config = nodes_telemetry::TracingConfig::try_from_env()?;
    let _tracing_handle = nodes_telemetry::initialize_tracing(&tracing_config)?;
    bitservice_peer::metrics::describe_metrics();
    tracing::info!(
        "{} {} ({})",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
        option_env!("GIT_HASH").unwrap_or(git_version!(fallback = "UNKNOWN"))
    );

    let result = bitservice_peer::start(BitservicePeerConfig::parse()).await;
    match result {
        Ok(()) => {
            tracing::info!("good night!");
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            // we don't want to double print the error therefore we just return FAILURE
            tracing::error!("{err:?}");
            Ok(ExitCode::FAILURE)
        }
    }
}
