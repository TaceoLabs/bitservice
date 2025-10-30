use std::{path::PathBuf, time::Duration};

use bitservice_peer::config::BitservicePeerConfig;
use bitservice_server::config::BitserviceServerConfig;
use testcontainers::{ContainerAsync, ImageExt as _, runners::AsyncRunner as _};
use testcontainers_modules::postgres::Postgres;

pub async fn start_server() -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let url = "http://localhost:4321".to_string();
    let config = BitserviceServerConfig {
        environment: bitservice_server::config::Environment::Dev,
        bind_addr: "0.0.0.0:4321".parse().unwrap(),
        rp_bitservice_peers_config: dir.join("../rp_bitservice_peers_config.toml"),
        peer_request_timeout: Duration::from_secs(60),
        prune_write_interval: 128,
        max_num_read_tasks: 4096,
    };
    tokio::spawn(async move {
        let res = bitservice_server::start(config).await;
        eprintln!("peer server to start: {res:?}");
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if reqwest::get(url.clone() + "/health").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("can start");
    url
}

async fn start_peer(id: u8, db_url: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let url = format!("http://localhost:1{id:04}"); // set port based on id, e.g. 10001 for id 1
    let next_id = match id {
        0 => 1,
        1 => 2,
        _ => 0,
    };
    let config = BitservicePeerConfig {
        environment: bitservice_peer::config::Environment::Dev,
        bind_addr: format!("0.0.0.0:1{id:04}").parse().unwrap(),
        tcp_mpc_net_bind_addr: format!("0.0.0.0:11{id:03}").parse().unwrap(),
        party_id: id,
        next_peer: format!("ws://localhost:1{next_id:04}/api/v1/ws"),
        tcp_next_peer: format!("127.0.0.1:11{next_id:03}").parse().unwrap(),
        prev_peer_wait_timeout: Duration::from_secs(10),
        oblivious_map_read_circuit_path: dir.join("../oblivious_map_read.json"),
        oblivious_map_write_circuit_path: dir.join("../oblivious_map_write.json"),
        secret_key_path: dir.join(format!("../dev-keys/peer{id}.sk")),
        db_url: db_url.into(),
    };
    tokio::spawn(async move {
        let res = bitservice_peer::start(config).await;
        eprintln!("peer failed to start: {res:?}");
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if reqwest::get(url.clone() + "/health").await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("can start");
    url
}

pub async fn start_peers(db_urls: &[String; 3]) -> [String; 3] {
    [
        start_peer(0, &db_urls[0]).await,
        start_peer(1, &db_urls[1]).await,
        start_peer(2, &db_urls[2]).await,
    ]
}

pub async fn postgres_testcontainer() -> eyre::Result<(ContainerAsync<Postgres>, String)> {
    let container = Postgres::default().with_network("network").start().await?;
    let ip = container.get_bridge_ip_address().await?;
    let db_url = format!("postgres://postgres:postgres@{ip}:5432/postgres");
    Ok((container, db_url))
}
