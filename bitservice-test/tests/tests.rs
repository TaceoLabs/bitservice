use std::path::PathBuf;

use bitservice_client::Value;
use bitservice_test::postgres_testcontainer;
use co_noir_to_r1cs::noir::{r1cs, ultrahonk};
use rand::{Rng, SeedableRng as _, rngs::StdRng};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn nullifier_e2e_test() -> eyre::Result<()> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut rng = rand::thread_rng();
    let (_postgres_container0, db_url0) = postgres_testcontainer().await?;
    let (_postgres_container1, db_url1) = postgres_testcontainer().await?;
    let (_postgres_container2, db_url2) = postgres_testcontainer().await?;

    let rp_id = 0;
    let peer_public_keys = (0..3)
        .map(|i| {
            let bytes = std::fs::read(dir.join(format!("../dev-keys/peer{i}.pk")))?;
            let public_key = crypto_box::PublicKey::from_slice(&bytes)?;
            Ok(public_key)
        })
        .collect::<eyre::Result<Vec<_>>>()?
        .try_into()
        .expect("len is 3");
    let read_program = ultrahonk::get_program_artifact(dir.join("../oblivious_map_read.json"))?;
    let (_, read_pk, _) = r1cs::setup_r1cs(read_program, &mut StdRng::from_seed([0; 32]))?;
    let write_program = ultrahonk::get_program_artifact(dir.join("../oblivious_map_write.json"))?;
    let (_, write_pk, _) = r1cs::setup_r1cs(write_program, &mut StdRng::from_seed([0; 32]))?;

    println!("starting server...");
    let server_url = bitservice_test::start_server().await;

    println!("starting peers...");
    bitservice_test::start_peers(&[db_url0, db_url1, db_url2]).await;

    let client = bitservice_client::Client::new(
        reqwest::Client::new(),
        server_url,
        rp_id,
        peer_public_keys,
        read_pk.vk,
        write_pk.vk,
    );
    let key = rng.r#gen();

    println!("read");
    let value = client.read(key, rng.r#gen(), &mut rng).await?;
    assert_eq!(value, Value::NotBanned);

    println!("ban");
    client.ban(key, rng.r#gen(), rng.r#gen(), &mut rng).await?;

    println!("read after ban");
    let value = client.read(key, rng.r#gen(), &mut rng).await?;
    assert_eq!(value, Value::Banned);

    println!("unban");
    client
        .unban(key, rng.r#gen(), rng.r#gen(), &mut rng)
        .await?;

    println!("read after unban");
    let value = client.read(key, rng.r#gen(), &mut rng).await?;
    assert_eq!(value, Value::NotBanned);

    Ok(())
}
