use async_trait::async_trait;
use hiraeth_store::IamStore;

#[derive(Clone)]
pub struct SqliteIamStore {
    pool: sqlx::SqlitePool,
}

impl SqliteIamStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

#[async_trait]
impl IamStore for SqliteIamStore {}
