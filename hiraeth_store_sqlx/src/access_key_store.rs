use hiraeth_store::{
    StoreError,
    access_key_store::{AccessKey, AccessKeyStore},
};

pub struct SqliteAccessKeyStore {
    pool: sqlx::SqlitePool,
}

impl SqliteAccessKeyStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl AccessKeyStore for SqliteAccessKeyStore {
    async fn get_secret_key(&self, access_key: &str) -> Result<Option<AccessKey>, StoreError> {
        sqlx::query_as!(
            AccessKey,
            "SELECT key_id, secret_key, principal_id, created_at FROM access_keys WHERE key_id = ?",
            access_key
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))
    }

    async fn insert_secret_key(
        &mut self,
        access_key: &str,
        secret_key: &str,
        principal_id: i64,
    ) -> Result<(), StoreError> {
        sqlx::query!(
            "INSERT INTO access_keys (key_id, secret_key, principal_id) VALUES (?, ?, ?)",
            access_key,
            secret_key,
            principal_id
        )
        .execute(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqliteAccessKeyStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::{StoreError, access_key_store::AccessKeyStore};

    async fn test_store() -> (TempDir, SqliteAccessKeyStore) {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let db_url = format!(
            "sqlite://{}",
            temp_dir.path().join("store-test.sqlite").display()
        );
        let pool = get_store_pool(&db_url)
            .await
            .expect("pool should connect to temp sqlite db");
        run_migrations(&pool)
            .await
            .expect("migrations should run for temp sqlite db");

        (temp_dir, SqliteAccessKeyStore::new(&pool))
    }

    #[tokio::test]
    async fn run_migrations_creates_access_keys_table() {
        let (_temp_dir, store) = test_store().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'access_keys'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("access_keys table should exist after migrations");

        assert_eq!(table_name, "access_keys");
    }

    #[tokio::test]
    async fn insert_and_get_secret_key_round_trips_access_key() {
        let (_temp_dir, mut store) = test_store().await;

        store
            .insert_secret_key("AKIAIOSFODNN7EXAMPLE", "example-secret", 1)
            .await
            .expect("insert should succeed");

        let access_key = store
            .get_secret_key("AKIAIOSFODNN7EXAMPLE")
            .await
            .expect("lookup should succeed")
            .expect("access key should exist");

        assert_eq!(access_key.key_id, "AKIAIOSFODNN7EXAMPLE");
        assert_eq!(access_key.secret_key, "example-secret");
    }

    #[tokio::test]
    async fn get_secret_key_returns_none_for_missing_key() {
        let (_temp_dir, store) = test_store().await;

        let access_key = store
            .get_secret_key("missing-key")
            .await
            .expect("lookup should succeed");

        assert!(access_key.is_none());
    }

    #[tokio::test]
    async fn insert_secret_key_returns_storage_error_for_duplicate_key() {
        let (_temp_dir, mut store) = test_store().await;

        store
            .insert_secret_key("AKIAIOSFODNN7EXAMPLE", "example-secret", 1)
            .await
            .expect("first insert should succeed");

        let duplicate_insert = store
            .insert_secret_key("AKIAIOSFODNN7EXAMPLE", "other-secret", 1)
            .await;

        assert!(matches!(duplicate_insert, Err(StoreError::StorageFailure(_))));
    }
}
