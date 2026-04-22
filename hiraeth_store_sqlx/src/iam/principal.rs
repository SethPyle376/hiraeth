use hiraeth_store::{
    StoreError,
    iam::{Principal, PrincipalStore},
};

#[derive(Clone)]
pub struct SqlitePrincipalStore {
    pool: sqlx::SqlitePool,
}

impl SqlitePrincipalStore {
    pub fn new(pool: &sqlx::SqlitePool) -> Self {
        Self { pool: pool.clone() }
    }
}

impl PrincipalStore for SqlitePrincipalStore {
    async fn get_principal(&self, principal_id: i64) -> Result<Option<Principal>, StoreError> {
        sqlx::query_as!(
            Principal,
            "SELECT id, account_id, kind, name, created_at FROM principals WHERE id = ?",
            principal_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|err| StoreError::StorageFailure(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::SqlitePrincipalStore;
    use crate::{get_store_pool, run_migrations};
    use hiraeth_store::iam::PrincipalStore;

    async fn test_store() -> (TempDir, SqlitePrincipalStore) {
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

        (temp_dir, SqlitePrincipalStore::new(&pool))
    }

    #[tokio::test]
    async fn run_migrations_creates_principals_table() {
        let (_temp_dir, store) = test_store().await;

        let table_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'principals'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("principals table should exist after migrations");

        assert_eq!(table_name, "principals");
    }

    #[tokio::test]
    async fn get_principal_returns_seeded_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal_id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM principals WHERE account_id = ? AND kind = ? AND name = ?",
        )
        .bind("000000000000")
        .bind("user")
        .bind("test")
        .fetch_one(&store.pool)
        .await
        .expect("seeded principal id should exist");

        let principal = store
            .get_principal(principal_id)
            .await
            .expect("principal lookup should succeed")
            .expect("principal should exist");

        assert_eq!(principal.id, principal_id);
        assert_eq!(principal.account_id, "000000000000");
        assert_eq!(principal.kind, "user");
        assert_eq!(principal.name, "test");
    }

    #[tokio::test]
    async fn get_principal_returns_none_for_missing_principal() {
        let (_temp_dir, store) = test_store().await;

        let principal = store
            .get_principal(999)
            .await
            .expect("principal lookup should succeed");

        assert!(principal.is_none());
    }
}
