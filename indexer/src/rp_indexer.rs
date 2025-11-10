use std::net::SocketAddr;
use std::sync::LazyLock;

use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::Filter;
use alloy::sol;
use alloy::sol_types::SolEvent;
use ark_bn254::Fr;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use poseidon2::{Poseidon2, POSEIDON2_BN254_T2_PARAMS};
use semaphore_rs_hasher::Hasher;
use semaphore_rs_trees::lazy::{Canonical, LazyMerkleTree as MerkleTree};
use semaphore_rs_trees::proof::InclusionProof;
use semaphore_rs_trees::Branch;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::RwLock;

// Contract events
sol! {
    #[sol(rpc)]
    contract RpAccountRegistry {
        event AccountAdded(uint256 indexed accountIndex, uint256 identityCommitment);
        event AccountUpdated(uint256 indexed accountIndex, uint256 oldIdentityCommitment, uint256 newIdentityCommitment);
        event RootRecorded(uint256 indexed root, uint256 timestamp, uint256 indexed rootEpoch);
    }
}

// Tree configuration
// TODO: Important we need to always sync this with whatever constant is onchain..
const TREE_DEPTH: usize = 30;

// Hasher setup
static POSEIDON_HASHER: LazyLock<Poseidon2<Fr, 2, 5>> =
    LazyLock::new(|| Poseidon2::new(&POSEIDON2_BN254_T2_PARAMS));

struct PoseidonHasher {}

impl Hasher for PoseidonHasher {
    type Hash = U256;

    fn hash_node(left: &Self::Hash, right: &Self::Hash) -> Self::Hash {
        let left: Fr = left.try_into().unwrap();
        let right: Fr = right.try_into().unwrap();
        let mut input = [left, right];
        let feed_forward = input[0];
        POSEIDON_HASHER.permutation_in_place(&mut input);
        input[0] += feed_forward;
        input[0].into()
    }
}

pub static GLOBAL_TREE: LazyLock<RwLock<MerkleTree<PoseidonHasher, Canonical>>> =
    LazyLock::new(|| RwLock::new(MerkleTree::<PoseidonHasher>::new(TREE_DEPTH, U256::ZERO)));


#[derive(Debug, Clone)]
pub struct Config {
    pub db_url: String,
    pub rpc_url: String,
    pub registry_address: Address,
    pub start_block: u64,
    pub http_addr: SocketAddr,
}


//TODO: Temporary config add more robust configuration rather than environment vars..?
impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            db_url: std::env::var("DATABASE_URL")?,
            rpc_url: std::env::var("RPC_URL")?,
            registry_address: std::env::var("REGISTRY_ADDRESS")?.parse()?,
            start_block: std::env::var("START_BLOCK")
                .unwrap_or_else(|_| "0".to_string())
                .parse()?,
            http_addr: std::env::var("HTTP_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
                .parse()?,
        })
    }
}

// Global state for latest root
static LATEST_ROOT: LazyLock<RwLock<RootInfo>> = LazyLock::new(|| {
    RwLock::new(RootInfo {
        root: U256::ZERO,
        timestamp: 0,
        epoch: 0,
        block_number: 0,
    })
});

#[derive(Debug, Clone)]
struct RootInfo {
    root: U256,
    timestamp: u64,
    epoch: u64,
    block_number: u64,
}

// Main indexer entry point
pub async fn run_indexer() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;

    let pool = PgPoolOptions::new()
        .max_connections(10) //TODO: No idea here perhaps a config value..?
        .connect(&cfg.db_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!("Building merkle tree from database...");
    build_tree_from_db(&pool).await?;

    // Start HTTP server
    let http_pool = pool.clone();
    let http_addr = cfg.http_addr;
    tokio::spawn(async move {
        if let Err(e) = start_http_server(http_addr, http_pool).await {
            tracing::error!(?e, "HTTP server failed");
        }
    });

    index_events(&cfg, &pool).await
}

// Build the merkle tree from existing database entries
async fn build_tree_from_db(pool: &PgPool) -> anyhow::Result<()> {
    let rows = sqlx::query(
        "SELECT account_index, identity_commitment FROM accounts ORDER BY account_index ASC"
    )
    .fetch_all(pool)
    .await?;

    tracing::info!("Found {} rows in database", rows.len());

    let mut leaves: Vec<(usize, U256)> = Vec::with_capacity(rows.len());
    for row in rows {
        let account_index: String = row.get("account_index");
        let commitment: String = row.get("identity_commitment");

        let index: U256 = account_index.parse()?;
        if index == U256::ZERO {
            // TODO: Question: Will this ever happen..?
            tracing::warn!("Found account with zero index");
            continue; // Skip zero index
        }

        // Account indices start at 1, tree indices start at 0
        let tree_index = index.as_limbs()[0] as usize - 1;
        let leaf_value: U256 = commitment.parse()?;

        leaves.push((tree_index, leaf_value));
    }

    // Build new tree with all leaves
    let mut new_tree = MerkleTree::<PoseidonHasher>::new(TREE_DEPTH, U256::ZERO);
    for (idx, value) in leaves {
        new_tree = new_tree.update_with_mutation(idx, &value);
    }

    let root = new_tree.root();

    // Update global tree
    {
        let mut tree = GLOBAL_TREE.write().await;
        *tree = new_tree;
    }

    tracing::info!(
        root = %format!("0x{:x}", root),
        "Merkle tree built successfully from DB"
    );

    Ok(())
}

// Update tree with a new or updated account
async fn update_tree_with_account(
    account_index: U256,
    identity_commitment: U256
) -> anyhow::Result<()> {
    if account_index == U256::ZERO {
        anyhow::bail!("Account index cannot be zero");
    }

    let tree_index = account_index.as_limbs()[0] as usize - 1;

    if tree_index >= (1usize << TREE_DEPTH) {
        anyhow::bail!("Account index {} out of range for tree depth {}", account_index, TREE_DEPTH);
    }

    let mut tree = GLOBAL_TREE.write().await;
    take_mut::take(&mut *tree, |tree| {
        tree.update_with_mutation(tree_index, &identity_commitment)
    });

    Ok(())
}

async fn index_events(cfg: &Config, pool: &PgPool) -> anyhow::Result<()> {
    let provider = ProviderBuilder::new()
        .connect_http(cfg.rpc_url.parse()?);

    let mut from_block = load_checkpoint(pool).await?.unwrap_or(cfg.start_block);

    loop {
        let to_block = provider.get_block_number().await?;

        if from_block > to_block {
            // We're caught up, wait before checking again
            tokio::time::sleep(std::time::Duration::from_secs(12)).await;
            continue;
        }

        // Process batch of blocks
        let batch_end = (from_block + 1000).min(to_block);

        let filter = Filter::new()
            .address(cfg.registry_address)
            .from_block(from_block)
            .to_block(batch_end);

        let logs = provider.get_logs(&filter).await?;

        if !logs.is_empty() {
            tracing::info!(
                count = logs.len(),
                from = from_block,
                to = batch_end,
                "processing logs"
            );
        }

        for log in logs {
            if let Err(e) = process_log(pool, &log).await {
                tracing::error!(?e, ?log, "failed to process log");
            }
        }

        save_checkpoint(pool, batch_end).await?;
        from_block = batch_end + 1;
    }
}

async fn process_log(pool: &PgPool, log: &alloy::rpc::types::Log) -> anyhow::Result<()> {
    if log.topics().is_empty() {
        return Ok(());
    }

    let sig = log.topics()[0];
    let block_number = log.block_number.unwrap_or(0);
    let tx_hash = format!("{:?}", log.transaction_hash.unwrap_or_default());

    if sig == RpAccountRegistry::AccountAdded::SIGNATURE_HASH {
        let event = RpAccountRegistry::AccountAdded::decode_log(log.log_decode()?, true)?;

        sqlx::query(
            r#"INSERT INTO accounts (account_index, identity_commitment, block_number, tx_hash)
               VALUES ($1, $2, $3, $4)
               ON CONFLICT (account_index) DO NOTHING"#
        )
        .bind(event.accountIndex.to_string())
        .bind(event.identityCommitment.to_string())
        .bind(block_number as i64)
        .bind(&tx_hash)
        .execute(pool)
        .await?;

        // Update merkle tree
        if let Err(e) = update_tree_with_account(event.accountIndex, event.identityCommitment).await {
            tracing::error!(?e, "Failed to update tree for AccountAdded");
        }

        tracing::info!(
            account_index = %event.accountIndex,
            "Account added"
        );

    } else if sig == RpAccountRegistry::AccountUpdated::SIGNATURE_HASH {
        let event = RpAccountRegistry::AccountUpdated::decode_log(log.log_decode()?, true)?;

        sqlx::query(
            r#"UPDATE accounts
               SET identity_commitment = $2,
                   block_number = $3,
                   tx_hash = $4,
                   updated_at = NOW()
               WHERE account_index = $1"#
        )
        .bind(event.accountIndex.to_string())
        .bind(event.newIdentityCommitment.to_string())
        .bind(block_number as i64)
        .bind(&tx_hash)
        .execute(pool)
        .await?;

        // Log the update event
        sqlx::query(
            r#"INSERT INTO account_updates
               (account_index, old_commitment, new_commitment, block_number, tx_hash)
               VALUES ($1, $2, $3, $4, $5)"#
        )
        .bind(event.accountIndex.to_string())
        .bind(event.oldIdentityCommitment.to_string())
        .bind(event.newIdentityCommitment.to_string())
        .bind(block_number as i64)
        .bind(&tx_hash)
        .execute(pool)
        .await?;

        // Update merkle tree
        if let Err(e) = update_tree_with_account(event.accountIndex, event.newIdentityCommitment).await {
            tracing::error!(?e, "Failed to update tree for AccountUpdated");
        }

        tracing::info!(
            account_index = %event.accountIndex,
            "Account updated"
        );

    } else if sig == RpAccountRegistry::RootRecorded::SIGNATURE_HASH {
        let event = RpAccountRegistry::RootRecorded::decode_log(log.log_decode()?, true)?;

        sqlx::query(
            r#"INSERT INTO roots (root, timestamp, epoch, block_number, tx_hash)
               VALUES ($1, $2, $3, $4, $5)"#
        )
        .bind(event.root.to_string())
        .bind(event.timestamp as i64)
        .bind(event.rootEpoch as i64)
        .bind(block_number as i64)
        .bind(&tx_hash)
        .execute(pool)
        .await?;

        // Update global state
        let mut root_info = LATEST_ROOT.write().await;
        *root_info = RootInfo {
            root: event.root,
            timestamp: event.timestamp,
            epoch: event.rootEpoch,
            block_number,
        };

        // Verify our computed root matches the contract's root
        let our_root = {
            let tree = GLOBAL_TREE.read().await;
            tree.root()
        };

        if our_root != event.root {
            tracing::warn!(
                contract_root = %event.root,
                computed_root = %our_root,
                "Root mismatch - our computed root differs from contract"
            );
        } else {
            tracing::info!(
                root = %event.root,
                epoch = event.rootEpoch,
                "Root recorded and verified"
            );
        }
    }

    Ok(())
}

async fn load_checkpoint(pool: &PgPool) -> anyhow::Result<Option<u64>> {
    let row: Option<(i64,)> = sqlx::query_as(
        "SELECT last_block FROM checkpoints WHERE name = 'indexer' LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(block,)| block as u64))
}

async fn save_checkpoint(pool: &PgPool, block: u64) -> anyhow::Result<()> {
    sqlx::query(
        r#"INSERT INTO checkpoints (name, last_block)
           VALUES ('indexer', $1)
           ON CONFLICT (name) DO UPDATE
           SET last_block = EXCLUDED.last_block"#
    )
    .bind(block as i64)
    .execute(pool)
    .await?;

    Ok(())
}

// HTTP API endpoints
async fn start_http_server(addr: SocketAddr, pool: PgPool) -> anyhow::Result<()> {
    let app = axum::Router::new()
        .route("/health", axum::routing::get(health))
        .route("/latest-root", axum::routing::get(get_latest_root))
        .route("/account/:index", axum::routing::get(get_account))
        .route("/proof/:index", axum::routing::get(get_inclusion_proof))
        .route("/roots", axum::routing::get(get_roots))
        .route("/stats", axum::routing::get(get_stats))
        .with_state(pool);

    tracing::info!(%addr, "HTTP server listening");
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "status": "healthy",
        "service": "rp-indexer"
    }))
}

async fn get_latest_root() -> impl IntoResponse {
    let root_info = LATEST_ROOT.read().await;
    let tree = GLOBAL_TREE.read().await;
    let computed_root = tree.root();

    axum::Json(serde_json::json!({
        "root": format!("0x{:x}", root_info.root),
        "computed_root": format!("0x{:x}", computed_root),
        "timestamp": root_info.timestamp,
        "epoch": root_info.epoch,
        "block_number": root_info.block_number
    }))
}

// Generate an inclusion proof for a particular account
async fn get_inclusion_proof(
    Path(index): Path<String>,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let account_index: U256 = match index.parse() {
        Ok(idx) => idx,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": "Invalid account index"
                }))
            ).into_response();
        }
    };

    if account_index == U256::ZERO {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "Account index cannot be zero"
            }))
        ).into_response();
    }

    // Get account data from database
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT identity_commitment FROM accounts WHERE account_index = $1"
    )
    .bind(account_index.to_string())
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    if row.is_none() {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": "Account not found"
            }))
        ).into_response();
    }

    let identity_commitment: U256 = match row.unwrap().0.parse() {
        Ok(c) => c,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": "Invalid commitment in database"
                }))
            ).into_response();
        }
    };

    let tree_index = account_index.as_limbs()[0] as usize - 1;

    if tree_index >= (1usize << TREE_DEPTH) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({
                "error": "Account index out of range"
            }))
        ).into_response();
    }

    // Generate proof from the merkle tree
    let tree = GLOBAL_TREE.read().await;
    let proof = tree.proof(tree_index);
    let root = tree.root();

    // Convert proof to array of siblings
    let siblings: Vec<String> = proof.0.iter().map(|branch| {
        match branch {
            Branch::Left(sibling) => format!("0x{:064x}", sibling),
            Branch::Right(sibling) => format!("0x{:064x}", sibling),
        }
    }).collect();

    // Verify proof is valid
    let is_valid = verify_proof_internal(&proof, tree_index, &identity_commitment, &root);

    axum::Json(serde_json::json!({
        "account_index": account_index.to_string(),
        "tree_index": tree_index,
        "identity_commitment": format!("0x{:064x}", identity_commitment),
        "root": format!("0x{:064x}", root),
        "siblings": siblings,
        "depth": TREE_DEPTH,
        "is_valid": is_valid
    })).into_response()
}

// Internal helper to verify a proof
// TODO: Should we use this one or the stateless onchain version as well? @Franco @Fabian
fn verify_proof_internal(
    proof: &InclusionProof<PoseidonHasher>,
    index: usize,
    leaf: &U256,
    expected_root: &U256
) -> bool {
    let mut hash = *leaf;

    for (i, branch) in proof.0.iter().enumerate() {
        let bit = (index >> i) & 1;
        hash = match branch {
            Branch::Left(sibling) if bit == 1 => PoseidonHasher::hash_node(sibling, &hash),
            Branch::Right(sibling) if bit == 0 => PoseidonHasher::hash_node(&hash, sibling),
            _ => return false,
        };
    }

    hash == *expected_root
}

async fn get_account(
    Path(index): Path<String>,
    State(pool): State<PgPool>,
) -> impl IntoResponse {
    let account_index: U256 = match index.parse() {
        Ok(idx) => idx,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({
                    "error": "Invalid account index"
                }))
            ).into_response();
        }
    };

    let row: Option<(String, i64, String, i64)> = sqlx::query_as(
        r#"SELECT identity_commitment, block_number, tx_hash,
           EXTRACT(EPOCH FROM created_at)::bigint as created_at
           FROM accounts WHERE account_index = $1"#
    )
    .bind(account_index.to_string())
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    match row {
        Some((commitment, block, tx_hash, created_at)) => {
            axum::Json(serde_json::json!({
                "account_index": account_index.to_string(),
                "identity_commitment": commitment,
                "block_number": block,
                "tx_hash": tx_hash,
                "created_at": created_at
            })).into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": "Account not found"
            }))
        ).into_response()
    }
}

async fn get_roots(State(pool): State<PgPool>) -> impl IntoResponse {
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        r#"SELECT root, timestamp, epoch, block_number
           FROM roots
           ORDER BY epoch DESC
           LIMIT 100"#
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_default();

    let roots: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(root, timestamp, epoch, block)| {
            serde_json::json!({
                "root": root,
                "timestamp": timestamp,
                "epoch": epoch,
                "block_number": block
            })
        })
        .collect();

    axum::Json(serde_json::json!({
        "roots": roots
    }))
}

async fn get_stats(State(pool): State<PgPool>) -> impl IntoResponse {
    let total_accounts: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM accounts"
    )
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    let total_updates: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM account_updates"
    )
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    let total_roots: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*) FROM roots"
    )
    .fetch_optional(&pool)
    .await
    .ok()
    .flatten();

    axum::Json(serde_json::json!({
        "total_accounts": total_accounts.map(|(c,)| c).unwrap_or(0),
        "total_updates": total_updates.map(|(c,)| c).unwrap_or(0),
        "total_roots": total_roots.map(|(c,)| c).unwrap_or(0)
    }))
}
