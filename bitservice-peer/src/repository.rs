use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use eyre::Context as _;
use oblivious_linear_scan_map::LinearScanObliviousMap;
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

    pub(crate) async fn load_map(&self) -> eyre::Result<Option<LinearScanObliviousMap>> {
        let row = sqlx::query("SELECT data FROM map WHERE id = 0")
            .fetch_optional(&self.pool)
            .await?;
        if let Some(row) = row {
            let data = row.get::<Vec<u8>, _>("data");
            Ok(Some(LinearScanObliviousMap::deserialize_uncompressed(
                data.as_slice(),
            )?))
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn store_map(&self, map: &LinearScanObliviousMap) -> eyre::Result<()> {
        let mut data = Vec::new();
        map.serialize_uncompressed(&mut data)?;
        sqlx::query(
            "
            INSERT INTO map (id, data)
            VALUES (0, $1)
            ON CONFLICT(id)
            DO UPDATE SET
                data = EXCLUDED.data;
            ",
        )
        .bind(data)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
