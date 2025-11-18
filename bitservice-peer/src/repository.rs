use eyre::Context as _;
use oblivious_linear_scan_map::{Groth16Material, LinearScanObliviousMap};
use sqlx::{PgPool, Row, migrate::Migrator, postgres::PgPoolOptions};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct DbPool {
    pub(crate) pool: PgPool,
}

impl DbPool {
    pub async fn open(db_url: &str) -> eyre::Result<Self> {
        let pool = PgPoolOptions::new()
            .connect(db_url)
            .await
            .context("while connecting to DB")?;
        let pool = Self::from_pool(pool).await?;
        Ok(pool)
    }

    pub(crate) async fn from_pool(pool: PgPool) -> eyre::Result<Self> {
        MIGRATOR.run(&pool).await.context("while migrating DB")?;
        Ok(Self { pool })
    }

    pub(crate) async fn load_or_init_map(
        &self,
        read_groth16: Groth16Material,
        write_groth16: Groth16Material,
    ) -> eyre::Result<LinearScanObliviousMap> {
        let row = sqlx::query("SELECT data FROM map WHERE id = 0")
            .fetch_optional(&self.pool)
            .await?;
        if let Some(row) = row {
            tracing::debug!("loading map from db");
            let data = row.get::<Vec<u8>, _>("data");
            let oblivious_map = LinearScanObliviousMap::from_dump(
                data.as_slice(),
                ark_serialize::Compress::No,
                ark_serialize::Validate::No,
                read_groth16,
                write_groth16,
            )?;
            Ok(oblivious_map)
        } else {
            tracing::debug!("init empty map in db");
            let oblivious_map = LinearScanObliviousMap::new(read_groth16, write_groth16);
            self.store_map(&oblivious_map).await?;
            Ok(oblivious_map)
        }
    }

    pub(crate) async fn store_map(
        &self,
        oblivious_map: &LinearScanObliviousMap,
    ) -> eyre::Result<()> {
        let mut data = Vec::new();
        oblivious_map.dump(&mut data, ark_serialize::Compress::No)?;
        sqlx::query(
            "
            INSERT INTO map (id, data)
            VALUES (0, $1)
            ON CONFLICT(id)
            DO UPDATE SET data = EXCLUDED.data;
            ",
        )
        .bind(data)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
